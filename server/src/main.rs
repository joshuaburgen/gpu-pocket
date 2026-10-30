mod config;
mod gpu;

use anyhow::{bail, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use config::{Config, Selection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

const GPU_IDS: [u32; 2] = [0, 1];

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    token: Arc<String>,
    demo: bool,
    mutation: Arc<Mutex<()>>,
    demo_active: Arc<Mutex<BTreeMap<u32, Selection>>>,
    demo_node: Arc<Mutex<String>>,
    client: reqwest::Client,
}

struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, error.to_string())
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
fn bad(message: impl ToString) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.to_string())
}

async fn authorize(State(app): State<App>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !bool::from(supplied.as_bytes().ct_eq(app.token.as_bytes())) {
        return ApiError(StatusCode::UNAUTHORIZED, "invalid bearer token".into()).into_response();
    }
    next.run(request).await
}

fn router(app: App) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/catalog", get(catalog))
        .route("/api/v1/selection", put(save))
        .route("/api/v1/miner", post(action))
        .route("/api/v1/gpus/{gpu_id}/selection", put(save_gpu))
        .route("/api/v1/gpus/{gpu_id}/miner", post(action_gpu))
        .route("/api/v1/proxy", get(proxy_status).put(proxy_select))
        .layer(DefaultBodyLimit::max(4096))
        .route_layer(middleware::from_fn_with_state(app.clone(), authorize))
        .with_state(app)
}

fn unit_name(gpu_id: u32) -> Result<String> {
    if !GPU_IDS.contains(&gpu_id) {
        bail!("gpu_id must be 0 or 1");
    }
    Ok(format!("gpu-pocket-miner@{gpu_id}.service"))
}

async fn service_state(app: &App, gpu_id: u32) -> Result<String> {
    let unit = unit_name(gpu_id)?;
    if app.demo {
        return Ok(if app.demo_active.lock().await.contains_key(&gpu_id) {
            "active"
        } else {
            "inactive"
        }
        .into());
    }
    Ok(gpu::output(
        "/usr/bin/systemctl",
        &["show", &unit, "--property=ActiveState", "--value"],
    )
    .await?
    .trim()
    .into())
}

async fn status(State(app): State<App>) -> ApiResult {
    let metrics = if app.demo {
        Ok(gpu::demo())
    } else {
        gpu::collect().await
    };
    let mut errors = vec![];
    let gpus = match metrics {
        Ok(gpus) => gpus,
        Err(e) => {
            errors.push(e.to_string());
            vec![]
        }
    };
    let mut gpu_controls = BTreeMap::new();
    for gpu_id in GPU_IDS {
        let service = match service_state(&app, gpu_id).await {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("GPU{gpu_id} service: {error}"));
                "unknown".into()
            }
        };
        let desired = match read_desired(&app, gpu_id) {
            Ok(selection) => selection,
            Err(error) => {
                errors.push(format!("GPU{gpu_id} desired selection: {error}"));
                None
            }
        };
        let active = if app.demo {
            app.demo_active.lock().await.get(&gpu_id).cloned()
        } else if service == "active" {
            app.config.read_gpu_selection("active", gpu_id).ok()
        } else {
            None
        };
        gpu_controls.insert(
            gpu_id.to_string(),
            json!({"service":service,"desired":desired,"active":active}),
        );
    }
    let service = gpu_controls["0"]["service"].clone();
    let desired = gpu_controls["0"]["desired"].clone();
    let active = gpu_controls["0"]["active"].clone();
    Ok(Json(
        json!({"host": "2-titan", "demo": app.demo, "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        "gpus": gpus, "service": service, "desired": desired, "active": active,
        "gpu_controls":gpu_controls,"control_enabled": app.config.control_enabled, "errors": errors}),
    ))
}

fn read_desired(app: &App, gpu_id: u32) -> Result<Option<Selection>> {
    let file = if gpu_id == 0 {
        "desired.json".to_string()
    } else {
        format!("gpu{gpu_id}-desired.json")
    };
    if !app.config.state_dir.join(&file).exists() {
        return Ok(None);
    }
    let desired = app.config.read_gpu_selection("desired", gpu_id)?;
    app.config.resolve(&desired)?;
    Ok(Some(desired))
}

async fn catalog(State(app): State<App>) -> ApiResult {
    // Only public fields leave the server. Binary paths and controller credentials stay local.
    let profiles: Vec<_> = app
        .config
        .profiles
        .iter()
        .map(|p| json!({"id":p.id,"label":p.label,"pools":p.pools}))
        .collect();
    Ok(Json(json!({"profiles": profiles})))
}

fn writable(app: &App) -> std::result::Result<(), ApiError> {
    if !app.config.control_enabled {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "control is disabled in server configuration".into(),
        ));
    }
    Ok(())
}

async fn save(State(app): State<App>, Json(selected): Json<Selection>) -> ApiResult {
    save_for_gpu(app, 0, selected).await
}

async fn save_gpu(
    State(app): State<App>,
    axum::extract::Path(gpu_id): axum::extract::Path<u32>,
    Json(selected): Json<Selection>,
) -> ApiResult {
    save_for_gpu(app, gpu_id, selected).await
}

async fn save_for_gpu(app: App, gpu_id: u32, mut selected: Selection) -> ApiResult {
    writable(&app)?;
    unit_name(gpu_id).map_err(bad)?;
    if selected.gpu_id != gpu_id {
        return Err(bad("gpu_id does not match the request path"));
    }
    let _guard = app.mutation.lock().await;
    app.config.resolve(&selected).map_err(bad)?;
    if let Some(power_limit_w) = selected.power_limit_w {
        let gpus = if app.demo {
            gpu::demo()
        } else {
            gpu::collect().await?
        };
        let gpu = gpus
            .iter()
            .find(|gpu| gpu.index == gpu_id)
            .ok_or_else(|| bad(format!("GPU{gpu_id} not found")))?;
        let min = gpu.power_min_limit_w.ok_or_else(|| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "GPU power limits unavailable".into(),
            )
        })?;
        let max = gpu.power_max_limit_w.ok_or_else(|| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "GPU power limits unavailable".into(),
            )
        })?;
        if (power_limit_w as f64) < min.ceil() || (power_limit_w as f64) > max.floor() {
            return Err(bad(format!(
                "power_limit_w must be between {} and {} for GPU{gpu_id}",
                min.ceil(),
                max.floor()
            )));
        }
    }
    let revision = read_desired(&app, gpu_id)?.map_or(0, |s| s.revision);
    if selected.revision != revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "selection changed; refresh before saving".into(),
        ));
    }
    selected.revision = revision
        .checked_add(1)
        .ok_or_else(|| bad("revision overflow"))?;
    app.config.save_gpu_selection("desired", &selected)?;
    tracing::info!(gpu_id, revision = selected.revision, "selection staged");
    Ok(Json(json!({"selection":selected,"applied":false})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    operation: Operation,
    revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Operation {
    Start,
    Restart,
    Stop,
}

async fn action(State(app): State<App>, Json(action): Json<Action>) -> ApiResult {
    action_for_gpu(app, 0, action).await
}

async fn action_gpu(
    State(app): State<App>,
    axum::extract::Path(gpu_id): axum::extract::Path<u32>,
    Json(action): Json<Action>,
) -> ApiResult {
    action_for_gpu(app, gpu_id, action).await
}

async fn action_for_gpu(app: App, gpu_id: u32, action: Action) -> ApiResult {
    writable(&app)?;
    let unit = unit_name(gpu_id).map_err(bad)?;
    let _guard = app.mutation.lock().await;
    let selection = read_desired(&app, gpu_id)?;
    if selection.as_ref().map_or(0, |s| s.revision) != action.revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "selection changed; refresh before applying".into(),
        ));
    }
    let operation = match action.operation {
        Operation::Start => "start",
        Operation::Restart => "restart",
        Operation::Stop => "stop",
    };
    if operation != "stop" {
        let s = selection
            .as_ref()
            .ok_or_else(|| bad("save a selection first"))?;
        app.config.resolve(s).map_err(bad)?;
        if !app.demo {
            let (profile, _) = app.config.resolve(s).map_err(bad)?;
            let path = app
                .config
                .gpu_root
                .join(&profile.binary)
                .canonicalize()
                .map_err(anyhow::Error::from)?;
            if !path.starts_with(
                app.config
                    .gpu_root
                    .canonicalize()
                    .map_err(anyhow::Error::from)?,
            ) {
                return Err(bad("binary escaped gpu_root"));
            }
            gpu::ensure_gpu_owned(gpu_id)
                .await
                .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
        }
    }
    if app.demo {
        let mut active = app.demo_active.lock().await;
        if operation == "stop" {
            active.remove(&gpu_id);
        } else if operation == "restart" || !active.contains_key(&gpu_id) {
            app.config
                .save_gpu_selection("applied", selection.as_ref().unwrap())?;
            active.insert(gpu_id, selection.unwrap());
        }
    } else {
        // Automatic systemd recovery must reuse the last explicitly applied version.
        // Merely staging another wallet must never change a later crash restart.
        if operation != "stop"
            && (operation == "restart" || service_state(&app, gpu_id).await? != "active")
        {
            app.config
                .save_gpu_selection("applied", selection.as_ref().unwrap())?;
        }
        gpu::output(
            "/usr/bin/sudo",
            &["-n", "/usr/bin/systemctl", operation, &unit],
        )
        .await?;
        let actual = service_state(&app, gpu_id).await?;
        if (operation == "stop" && actual != "inactive")
            || (operation != "stop" && actual != "active")
        {
            return Err(ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("service is {actual}; inspect journal before retrying"),
            ));
        }
    }
    tracing::info!(gpu_id, operation, "miner control completed");
    Ok(Json(
        json!({"ok": true, "gpu_id":gpu_id,"operation":operation,"demo":app.demo}),
    ))
}

fn proxy_url(app: &App) -> Result<reqwest::Url> {
    let proxy = app
        .config
        .proxy
        .as_ref()
        .context("proxy controller is not configured")?;
    let mut url = reqwest::Url::parse(&proxy.endpoint)?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid controller URL"))?
        .clear()
        .push("proxies")
        .push(&proxy.group);
    Ok(url)
}
fn proxy_request(app: &App, method: reqwest::Method) -> Result<reqwest::RequestBuilder> {
    let token =
        std::env::var("GPU_POCKET_PROXY_SECRET").context("GPU_POCKET_PROXY_SECRET is missing")?;
    Ok(app
        .client
        .request(method, proxy_url(app)?)
        .bearer_auth(token))
}
async fn read_proxy(app: &App) -> Result<Value> {
    if app.demo {
        return Ok(
            json!({"type":"Selector","now":app.demo_node.lock().await.clone(),"all":["DEMO-HK","DEMO-SG"]}),
        );
    }
    let response = proxy_request(app, reqwest::Method::GET)?.send().await?;
    if !response.status().is_success() {
        bail!("proxy controller returned HTTP {}", response.status());
    }
    Ok(response.json().await?)
}
async fn proxy_status(State(app): State<App>) -> ApiResult {
    let value = read_proxy(&app).await?;
    Ok(Json(
        json!({"group":app.config.proxy.as_ref().map(|p|p.group.as_str()).unwrap_or("DEMO"),
        "type":value["type"],"current":value["now"],"nodes":value["all"],
        "selectable":app.config.control_enabled && value["type"] == "Selector", "demo":app.demo}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    name: String,
}
async fn proxy_select(State(app): State<App>, Json(node): Json<Node>) -> ApiResult {
    writable(&app)?;
    let _guard = app.mutation.lock().await;
    let value = read_proxy(&app).await?;
    if value["type"] != "Selector" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "group is not Selector; Fallback is automatic and cannot be manually selected".into(),
        ));
    }
    if node.name == "DIRECT"
        || !value["all"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|n| n.as_str() == Some(&node.name)))
    {
        return Err(bad("node is not an allowed member of the configured group"));
    }
    if !app.demo {
        let response = proxy_request(&app, reqwest::Method::PUT)?
            .json(&json!({"name": node.name}))
            .send()
            .await
            .map_err(anyhow::Error::from)?;
        if !response.status().is_success() {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "proxy selection failed".into(),
            ));
        }
    } else {
        *app.demo_node.lock().await = node.name;
    }
    Ok(Json(json!({"ok":true,"demo":app.demo})))
}

fn launch(config: &Config, gpu_id: u32) -> Result<()> {
    unit_name(gpu_id)?;
    let selection = config.read_gpu_selection("applied", gpu_id)?;
    if selection.gpu_id != gpu_id {
        bail!("applied selection does not match the miner GPU");
    }
    let (profile, pool) = config.resolve(&selection)?;
    let binary = config.gpu_root.join(&profile.binary).canonicalize()?;
    if !binary.starts_with(config.gpu_root.canonicalize()?) {
        bail!("binary escaped gpu_root");
    }
    let tls = if pool.tls { "true" } else { "false" };
    let mut command = std::process::Command::new(&binary);
    command.current_dir(binary.parent().context("binary has no parent")?);
    if profile.miner == "srb-pearl" {
        let api_port = (21550 + gpu_id).to_string();
        command.args([
            "--disable-cpu",
            "--gpu-id",
            &gpu_id.to_string(),
            "--algorithm",
            "pearlhash",
            "--pool",
            &pool.address,
            "--wallet",
            &selection.wallet,
            "--tls",
            tls,
            "--retry-time",
            "5",
            "--give-up-limit",
            "3",
            "--api-enable",
            "--api-port",
            &api_port,
        ]);
        if let Some(power_limit_w) = selection.power_limit_w {
            let power_argument = format!("--gpu-plimit{gpu_id}");
            let power_value = power_limit_w.to_string();
            command.args([power_argument.as_str(), power_value.as_str()]);
        }
    } else {
        command.args([
            "--config",
            "./research-octopus.cfg",
            "--algo",
            "OCTOPUS",
            "--pool",
            &pool.address,
            "--user",
            &selection.wallet,
            "--pass",
            "x",
            "--tls",
            if pool.tls { "on" } else { "off" },
            "--dns-over-https",
            "0",
            "--devices",
            &gpu_id.to_string(),
        ]);
        if let Some(power_limit_w) = selection.power_limit_w {
            let limits = if gpu_id == 0 {
                format!("{power_limit_w},*")
            } else {
                format!("*,{power_limit_w}")
            };
            command.args(["--pl", &limits]);
        }
    }
    config.save_gpu_selection("active", &selection)?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().into())
    }
    #[cfg(not(unix))]
    {
        bail!("miner execution requires Linux");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<_> = std::env::args().collect();
    let miner = args.get(1).is_some_and(|s| s == "miner");
    let path = args
        .get(if miner { 2 } else { 1 })
        .map(String::as_str)
        .unwrap_or("server.toml");
    let config = Config::load(path)?;
    if miner {
        let gpu_id = args.get(3).map_or(Ok(0), |value| value.parse::<u32>())?;
        return launch(&config, gpu_id);
    }
    let token = std::env::var("GPU_POCKET_TOKEN")
        .context("set GPU_POCKET_TOKEN to at least 32 random characters")?;
    if token.len() < 32 || !token.is_ascii() {
        bail!("GPU_POCKET_TOKEN must contain at least 32 ASCII characters");
    }
    let demo = std::env::var("GPU_POCKET_MODE").as_deref() == Ok("demo");
    let address = config.listen.parse::<std::net::SocketAddr>()?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let app = App {
        config: Arc::new(config),
        token: Arc::new(token),
        demo,
        mutation: Arc::new(Mutex::new(())),
        demo_active: Arc::new(Mutex::new(BTreeMap::new())),
        demo_node: Arc::new(Mutex::new("DEMO-HK".into())),
        client,
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, demo, "GPU Pocket API listening");
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
