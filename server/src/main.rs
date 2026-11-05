mod ai;
mod config;
mod gpu;
mod push;

use anyhow::{bail, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use config::{Config, CpuMode, CpuPolicy, CpuSelection, GpuMode, GpuPolicy, Selection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

const GPU_IDS: [u32; 2] = [0, 1];
const AUTO_START_IDLE_SECONDS: u64 = 10 * 60;
const AUTO_START_UTILIZATION_THRESHOLD: f64 = 25.0;

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    token: Arc<String>,
    demo: bool,
    mutation: Arc<Mutex<()>>,
    demo_active: Arc<Mutex<BTreeMap<u32, Selection>>>,
    demo_cpu_active: Arc<Mutex<Option<CpuSelection>>>,
    demo_node: Arc<Mutex<String>>,
    runtime: Arc<Mutex<BTreeMap<u32, RuntimeState>>>,
    process_watch: Arc<Mutex<ProcessWatchState>>,
    cpu_usage: Arc<Mutex<Option<(u64, u64)>>>,
    cpu_runtime: Arc<Mutex<CpuRuntime>>,
    push: Arc<push::PushService>,
    client: reqwest::Client,
    ai: Arc<Mutex<ai::AiState>>,
}

#[derive(Default)]
struct ProcessWatchState {
    initialized: bool,
    external: BTreeMap<(u32, u32, String), gpu::GpuProcess>,
}

#[derive(Default)]
struct RuntimeState {
    low_util_since: Option<Instant>,
    external_task: bool,
    memory_blocked: bool,
    last_message: Option<String>,
}

#[derive(Default)]
struct CpuRuntime {
    last_sample: Option<(u64, u64)>,
    low_usage_since: Option<Instant>,
    usage_percent: Option<u32>,
    last_message: Option<String>,
}

#[derive(Debug)]
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
        .merge(ai::routes())
        .route("/api/v1/status", get(status))
        .route("/api/v1/catalog", get(catalog))
        .route("/api/v1/selection", put(save))
        .route("/api/v1/miner", post(action))
        .route("/api/v1/cpu/selection", put(save_cpu_selection))
        .route("/api/v1/cpu/miner", post(action_cpu))
        .route(
            "/api/v1/cpu/policy",
            get(read_cpu_policy).put(save_cpu_policy),
        )
        .route("/api/v1/gpus/{gpu_id}/selection", put(save_gpu))
        .route("/api/v1/gpus/{gpu_id}/miner", post(action_gpu))
        .route(
            "/api/v1/gpus/{gpu_id}/policy",
            get(read_policy).put(save_policy),
        )
        .route(
            "/api/v1/push/token",
            put(register_push_token).delete(unregister_push_token),
        )
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

const CPU_UNIT: &str = "gpu-pocket-cpu.service";

#[derive(Default)]
struct CpuUsageSample {
    total: u64,
    idle: u64,
}

fn read_cpu_usage() -> Result<CpuUsageSample> {
    let mut total = 0u64;
    let mut idle = 0u64;
    let first = std::fs::read_to_string("/proc/stat")?
        .lines()
        .next()
        .context("missing aggregate /proc/stat row")?
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect::<Vec<_>>();
    for (index, value) in first.iter().enumerate() {
        total = total.saturating_add(*value);
        if index == 3 || index == 4 {
            idle = idle.saturating_add(*value);
        }
    }
    Ok(CpuUsageSample { total, idle })
}

async fn cpu_usage_percent(app: &App) -> Option<u32> {
    let current = match read_cpu_usage() {
        Ok(value) => value,
        Err(_) => return None,
    };
    let mut previous = app.cpu_usage.lock().await;
    let percent = previous.as_ref().and_then(|sample: &(u64, u64)| {
        let total_delta = current.total.saturating_sub(sample.0);
        let idle_delta = current.idle.saturating_sub(sample.1);
        100u64
            .saturating_mul(total_delta.saturating_sub(idle_delta))
            .checked_div(total_delta)
            .map(|percent| percent as u32)
    });
    *previous = Some((current.total, current.idle));
    percent
}

async fn cpu_service_state(app: &App) -> Result<String> {
    if app.demo {
        return Ok(if app.demo_cpu_active.lock().await.is_some() {
            "active"
        } else {
            "inactive"
        }
        .into());
    }
    let service = gpu::output(
        "/usr/bin/systemctl",
        &["show", CPU_UNIT, "--property=ActiveState", "--value"],
    )
    .await?
    .trim()
    .to_string();
    if service != "active" {
        if let Ok(processes) =
            gpu::output("/usr/bin/pgrep", &["-af", "research-srb|srbminer"]).await
        {
            if !processes.trim().is_empty()
                && processes.lines().any(|line| {
                    line.contains("--algorithm-cpu") && !line.contains("gpu-pocket-server")
                })
            {
                return Ok("active".into());
            }
        }
    }
    Ok(service)
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

async fn service_failure_reason(app: &App, unit: &str, state: &str) -> Result<Option<String>> {
    if app.demo || state != "failed" {
        return Ok(None);
    }
    let details = gpu::output(
        "/usr/bin/systemctl",
        &[
            "show",
            unit,
            "--property=Result",
            "--property=ExecMainStatus",
        ],
    )
    .await?;
    let properties: BTreeMap<_, _> = details
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let result = properties.get("Result").copied().unwrap_or("unknown");
    let exit_status = properties
        .get("ExecMainStatus")
        .copied()
        .unwrap_or("unknown");
    let fallback = format!("服务启动失败：{result}，退出码 {exit_status}");
    let journal = gpu::output(
        "/usr/bin/journalctl",
        &["--unit", unit, "-n", "30", "--no-pager", "-o", "cat"],
    )
    .await?;
    Ok(Some(
        journal
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| is_safe_failure_detail(line))
            .map(str::to_string)
            .unwrap_or(fallback),
    ))
}

fn is_safe_failure_detail(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if [
        "wallet",
        "password",
        "token",
        "authorization",
        "--user",
        "--pass",
    ]
    .iter()
    .any(|value| lower.contains(value))
    {
        return false;
    }
    line.starts_with("Error:")
        || line.starts_with("Caused by:")
        || [
            "failed to",
            "permission denied",
            "no such file",
            "unknown field",
        ]
        .iter()
        .any(|value| lower.contains(value))
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
    let process_snapshot = if app.demo {
        Some(Vec::new())
    } else {
        match gpu::processes(&app.config.gpu_root).await {
            Ok(processes) => Some(processes),
            Err(error) => {
                errors.push(format!("GPU process list: {error}"));
                None
            }
        }
    };
    let mut gpu_controls = BTreeMap::new();
    let mut policies = BTreeMap::new();
    for gpu_id in GPU_IDS {
        let service = match service_state(&app, gpu_id).await {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("GPU{gpu_id} service: {error}"));
                "unknown".into()
            }
        };
        let service_error = match service_failure_reason(
            &app,
            &unit_name(gpu_id)
                .map_err(|error| ApiError(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
            &service,
        )
        .await
        {
            Ok(error) => error,
            Err(error) => {
                errors.push(format!("GPU{gpu_id} service diagnosis: {error}"));
                Some(format!("服务启动失败，无法读取详细原因：{error}"))
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
            match app.config.read_gpu_selection("active", gpu_id) {
                Ok(selection) => Some(selection),
                Err(error) => {
                    errors.push(format!("GPU{gpu_id} active selection: {error}"));
                    None
                }
            }
        } else {
            None
        };
        let external_pids = if app.demo {
            Some(Vec::new())
        } else {
            match gpu::unmanaged_compute_pids(gpu_id).await {
                Ok(pids) => Some(pids),
                Err(error) => {
                    errors.push(format!("GPU{gpu_id} process status: {error}"));
                    None
                }
            }
        };
        let processes = process_snapshot.as_ref().map(|processes| {
            processes
                .iter()
                .filter(|process| process.gpu_id == gpu_id)
                .collect::<Vec<_>>()
        });
        let idle_seconds = if service == "active" {
            None
        } else {
            app.runtime
                .lock()
                .await
                .get(&gpu_id)
                .and_then(|state| state.low_util_since.map(|since| since.elapsed().as_secs()))
        };
        gpu_controls.insert(
            gpu_id.to_string(),
            json!({
                "service":service,
                "service_error":service_error,
                "desired":desired,
                "active":active,
                "external_pids":external_pids,
                "processes":processes,
                "idle_seconds":idle_seconds
            }),
        );
        let policy = match app.config.read_gpu_policy(gpu_id) {
            Ok(policy) => Some(policy),
            Err(error) => {
                errors.push(format!("GPU{gpu_id} policy: {error}"));
                None
            }
        };
        let mode = match app.config.read_gpu_mode(gpu_id) {
            Ok(mode) => Some(mode),
            Err(error) => {
                errors.push(format!("GPU{gpu_id} mode: {error}"));
                None
            }
        };
        let runtime = app.runtime.lock().await;
        let runtime_state = runtime.get(&gpu_id);
        policies.insert(
            gpu_id.to_string(),
            json!({
                "policy":policy,
                "mode":mode,
                "runtime":{
                    "low_util_seconds":runtime_state.and_then(|state| state.low_util_since.map(|since| since.elapsed().as_secs())),
                    "external_task":runtime_state.is_some_and(|state| state.external_task),
                    "memory_blocked":runtime_state.is_some_and(|state| state.memory_blocked),
                    "message":runtime_state.and_then(|state| state.last_message.clone())
                }
            }),
        );
    }
    let service = gpu_controls["0"]["service"].clone();
    let desired = gpu_controls["0"]["desired"].clone();
    let active = gpu_controls["0"]["active"].clone();
    let cpu_service = match cpu_service_state(&app).await {
        Ok(value) => value,
        Err(error) => {
            errors.push(format!("CPU miner service: {error}"));
            "unknown".into()
        }
    };
    let cpu_service_error = match service_failure_reason(&app, CPU_UNIT, &cpu_service).await {
        Ok(error) => error,
        Err(error) => {
            errors.push(format!("CPU miner service diagnosis: {error}"));
            Some(format!("CPU 服务启动失败，无法读取详细原因：{error}"))
        }
    };
    let cpu_desired = match read_cpu_desired(&app) {
        Ok(selection) => selection,
        Err(error) => {
            errors.push(format!("CPU desired selection: {error}"));
            None
        }
    };
    let cpu_active = if app.demo {
        app.demo_cpu_active.lock().await.clone()
    } else if cpu_service == "active" {
        match app.config.read_cpu_selection("active") {
            Ok(selection) => Some(selection),
            Err(_) => match app.config.read_cpu_selection("applied") {
                Ok(selection) => Some(selection),
                Err(error) => {
                    errors.push(format!("CPU active selection: {error}"));
                    None
                }
            },
        }
    } else {
        None
    };
    let cpu_usage_percent = cpu_usage_percent(&app).await;
    let cpu_policy = match app.config.read_cpu_policy() {
        Ok(policy) => Some(policy),
        Err(error) => {
            errors.push(format!("CPU policy: {error}"));
            None
        }
    };
    let cpu_mode = match app.config.read_cpu_mode() {
        Ok(mode) => Some(mode),
        Err(error) => {
            errors.push(format!("CPU mode: {error}"));
            None
        }
    };
    let cpu_runtime = app.cpu_runtime.lock().await;
    let cpu_low_usage_seconds = cpu_runtime
        .low_usage_since
        .map(|since| since.elapsed().as_secs());
    let cpu_runtime_message = cpu_runtime.last_message.clone();
    Ok(Json(
        json!({"host": app.config.host_name.clone(), "demo": app.demo, "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        "gpus": gpus, "service": service, "desired": desired, "active": active,
        "gpu_controls":gpu_controls,"policies":policies,
        "cpu_mining":{"service":cpu_service,"service_error":cpu_service_error,"desired":cpu_desired,"active":cpu_active,"usage_percent":cpu_usage_percent,
            "policy":cpu_policy,"mode":cpu_mode,
            "runtime":{"low_usage_seconds":cpu_low_usage_seconds,"message":cpu_runtime_message}},
        "push_configured":app.push.configured(),
        "control_enabled": app.config.control_enabled, "errors": errors}),
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
        .map(|p| json!({"id":p.id,"label":p.label,"miner":p.miner,"pools":p.pools}))
        .collect();
    let cpu_profiles: Vec<_> = app
        .config
        .cpu_profiles
        .iter()
        .map(|p| json!({"id":p.id,"label":p.label,"algorithm":p.algorithm,"pools":p.pools}))
        .collect();
    Ok(Json(
        json!({"profiles": profiles,"cpu_profiles":cpu_profiles,"manual_night_supported":true}),
    ))
}

fn read_cpu_desired(app: &App) -> Result<Option<CpuSelection>> {
    let path = app.config.state_dir.join("cpu-desired.json");
    if !path.exists() {
        return Ok(None);
    }
    let desired = app.config.read_cpu_selection("desired")?;
    app.config.resolve_cpu(&desired)?;
    Ok(Some(desired))
}

async fn save_cpu_selection(
    State(app): State<App>,
    Json(mut selected): Json<CpuSelection>,
) -> ApiResult {
    writable(&app)?;
    let _guard = app.mutation.lock().await;
    app.config.resolve_cpu(&selected).map_err(bad)?;
    let revision = read_cpu_desired(&app)?.map_or(0, |s| s.revision);
    if selected.revision != revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "selection changed; refresh before saving".into(),
        ));
    }
    selected.revision = revision
        .checked_add(1)
        .ok_or_else(|| bad("revision overflow"))?;
    app.config.save_cpu_selection("desired", &selected)?;
    tracing::info!(
        revision = selected.revision,
        threads_percent = selected.threads_percent,
        "CPU selection staged"
    );
    Ok(Json(json!({"selection":selected,"applied":false})))
}

async fn read_cpu_policy(State(app): State<App>) -> ApiResult {
    let policy = app.config.read_cpu_policy()?;
    let mode = app.config.read_cpu_mode()?;
    let runtime = app.cpu_runtime.lock().await;
    Ok(Json(json!({
        "policy":policy,
        "mode":mode,
        "runtime":{
            "low_usage_seconds":runtime.low_usage_since.map(|since| since.elapsed().as_secs()),
            "usage_percent":runtime.usage_percent,
            "message":runtime.last_message
        }
    })))
}

async fn save_cpu_policy(State(app): State<App>, Json(policy): Json<CpuPolicy>) -> ApiResult {
    writable(&app)?;
    app.config.save_cpu_policy(&policy).map_err(bad)?;
    Ok(Json(json!({"policy":app.config.read_cpu_policy()?})))
}

async fn action_cpu(State(app): State<App>, Json(action): Json<Action>) -> ApiResult {
    let mode = match action.mode.unwrap_or(GpuMode::Normal) {
        GpuMode::Normal => CpuMode::Normal,
        GpuMode::ManualNight => CpuMode::ManualNight,
        _ => return Err(bad("CPU miner mode must be normal or manual_night")),
    };
    apply_cpu_action(app, action, mode, true, false).await
}

async fn apply_cpu_action(
    app: App,
    action: Action,
    requested_mode: CpuMode,
    manual: bool,
    exit_manual_night: bool,
) -> ApiResult {
    if manual {
        writable(&app)?;
    }
    let _guard = app.mutation.lock().await;
    let current_mode = app.config.read_cpu_mode()?;
    if !manual
        && current_mode == CpuMode::ManualNight
        && !exit_manual_night
        && !(action.operation == Operation::Stop && requested_mode == CpuMode::ManualNight)
    {
        return Ok(Json(json!({"ok":true,"mode":current_mode,"skipped":true})));
    }
    let selection = read_cpu_desired(&app)?;
    if selection.as_ref().map_or(0, |s| s.revision) != action.revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "selection changed; refresh before applying".into(),
        ));
    }
    if action.operation != Operation::Stop
        && requested_mode == CpuMode::ManualNight
        && !is_china_night(SystemTime::now())?
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "manual CPU night mode can only start between Beijing 01:00 and 08:00".into(),
        ));
    }
    let operation = match action.operation {
        Operation::Start => "start",
        Operation::Restart => "restart",
        Operation::Stop => "stop",
    };
    if operation != "stop" {
        let selected = selection
            .as_ref()
            .ok_or_else(|| bad("save a CPU selection first"))?;
        let (profile, _) = app.config.resolve_cpu(selected).map_err(bad)?;
        if !app.demo {
            ensure_miner_binary(&app.config, &profile.binary)?;
        }
    }
    let mode_to_apply = if action.operation == Operation::Stop {
        if !manual && current_mode == CpuMode::ManualNight && !exit_manual_night {
            CpuMode::ManualNight
        } else {
            CpuMode::Normal
        }
    } else {
        requested_mode
    };
    app.config.save_cpu_mode(mode_to_apply)?;
    if app.demo {
        let mut active = app.demo_cpu_active.lock().await;
        if operation == "stop" {
            *active = None;
        } else if operation == "restart" || active.is_none() {
            app.config
                .save_cpu_selection("applied", selection.as_ref().unwrap())?;
            *active = selection;
        }
    } else {
        if operation != "stop"
            && (operation == "restart" || cpu_service_state(&app).await? != "active")
        {
            app.config
                .save_cpu_selection("applied", selection.as_ref().unwrap())?;
        }
        gpu::output(
            "/usr/bin/sudo",
            &["-n", "/usr/bin/systemctl", operation, CPU_UNIT],
        )
        .await?;
        let actual = cpu_service_state(&app).await?;
        if (operation == "stop" && actual != "inactive")
            || (operation != "stop" && actual != "active")
        {
            return Err(ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("CPU miner service is {actual}; inspect journal before retrying"),
            ));
        }
    }
    tracing::info!(operation, "CPU miner control completed");
    Ok(Json(
        json!({"ok":true,"operation":operation,"mode":mode_to_apply,"demo":app.demo}),
    ))
}

async fn auto_cpu_action(
    app: &App,
    operation: Operation,
    mode: CpuMode,
    exit_manual_night: bool,
) -> Result<()> {
    let selection = read_cpu_desired(app)?.context("no staged CPU selection")?;
    let action = Action {
        operation,
        revision: selection.revision,
        mode: Some(match mode {
            CpuMode::ManualNight => GpuMode::ManualNight,
            _ => GpuMode::Normal,
        }),
    };
    let _ = apply_cpu_action(app.clone(), action, mode, false, exit_manual_night)
        .await
        .map_err(|error| anyhow::anyhow!(error.1))?;
    Ok(())
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
    selected.power_limit_w = None;
    selected.gpu_intensity = None;
    if selected.gpu_id != gpu_id {
        return Err(bad("gpu_id does not match the request path"));
    }
    let _guard = app.mutation.lock().await;
    let (profile, _) = app.config.resolve(&selected).map_err(bad)?;
    let policy = app.config.read_gpu_policy(gpu_id)?;
    if policy.enabled
        && (policy.night_mode_enabled || policy.throttle_enabled)
        && profile.miner != "srb-pearl"
    {
        return Err(bad(
            "disable SRBMiner automation before selecting a different miner",
        ));
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

async fn read_policy(
    State(app): State<App>,
    axum::extract::Path(gpu_id): axum::extract::Path<u32>,
) -> ApiResult {
    unit_name(gpu_id).map_err(bad)?;
    let policy = app.config.read_gpu_policy(gpu_id)?;
    let mode = app.config.read_gpu_mode(gpu_id)?;
    Ok(Json(json!({"policy":policy,"mode":mode})))
}

async fn save_policy(
    State(app): State<App>,
    axum::extract::Path(gpu_id): axum::extract::Path<u32>,
    Json(policy): Json<GpuPolicy>,
) -> ApiResult {
    writable(&app)?;
    unit_name(gpu_id).map_err(bad)?;
    if policy.gpu_id != gpu_id {
        return Err(bad("gpu_id does not match the request path"));
    }
    if !(1..=31).contains(&policy.intensity) {
        return Err(bad("policy intensity must be between 1 and 31"));
    }
    if !(512..=16_384).contains(&policy.min_free_memory_mib) {
        return Err(bad("min_free_memory_mib must be between 512 and 16384"));
    }
    if policy.night_mode_enabled || policy.throttle_enabled {
        let selection = read_desired(&app, gpu_id)?
            .ok_or_else(|| bad("save a GPU selection before enabling automation"))?;
        let (profile, _) = app.config.resolve(&selection).map_err(bad)?;
        if profile.miner != "srb-pearl" {
            return Err(bad(
                "throttle and night automation currently require SRBMiner Pearl",
            ));
        }
    }
    app.config.save_gpu_policy(&policy)?;
    tracing::info!(
        gpu_id,
        enabled = policy.enabled,
        "GPU automation policy saved"
    );
    Ok(Json(json!({"policy":policy})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PushToken {
    token: String,
}

async fn register_push_token(State(app): State<App>, Json(token): Json<PushToken>) -> ApiResult {
    app.push.register_token(&token.token).await.map_err(bad)?;
    Ok(Json(
        json!({"registered":true,"push_configured":app.push.configured()}),
    ))
}

async fn unregister_push_token(State(app): State<App>, Json(token): Json<PushToken>) -> ApiResult {
    app.push.unregister_token(&token.token).await?;
    Ok(Json(json!({"registered":false})))
}

async fn policy_loop(app: App) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.tick().await;
    loop {
        interval.tick().await;
        if let Err(error) = policy_tick(&app).await {
            tracing::error!(%error, "GPU automation tick failed");
        }
    }
}

async fn process_monitor_loop(app: App) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.tick().await;
    loop {
        interval.tick().await;
        if let Err(error) = process_monitor_tick(&app).await {
            tracing::error!(%error, "GPU process monitor tick failed");
        }
    }
}

async fn cpu_policy_tick(app: &App, now: SystemTime) -> Result<()> {
    let policy = app.config.read_cpu_policy()?;
    let mode = app.config.read_cpu_mode()?;
    let service = cpu_service_state(app).await?;
    let in_night = is_china_night(now)?;

    if mode == CpuMode::ManualNight {
        if !in_night {
            auto_cpu_action(app, Operation::Stop, CpuMode::ManualNight, true).await?;
            let mut runtime = app.cpu_runtime.lock().await;
            runtime.low_usage_since = None;
            runtime.last_message = None;
        }
        return Ok(());
    }

    if !app.config.control_enabled {
        return Ok(());
    }

    if !policy.enabled {
        if service == "active" && matches!(mode, CpuMode::IdleAuto | CpuMode::Night) {
            auto_cpu_action(app, Operation::Stop, CpuMode::Normal, false).await?;
        }
        app.cpu_runtime.lock().await.low_usage_since = None;
        return Ok(());
    }

    if in_night && policy.night_mode_enabled {
        if service != "active" || mode != CpuMode::Night {
            auto_cpu_action(
                app,
                if service == "active" {
                    Operation::Restart
                } else {
                    Operation::Start
                },
                CpuMode::Night,
                false,
            )
            .await?;
            notify_policy(
                app,
                0,
                "GPU Pocket CPU 深夜模式",
                "CPU 已按北京时间 01:00–08:00 启动深夜模式。",
            )
            .await;
        }
        app.cpu_runtime.lock().await.low_usage_since = None;
        return Ok(());
    }
    if service == "active" && mode == CpuMode::Night {
        auto_cpu_action(app, Operation::Stop, CpuMode::Normal, false).await?;
        notify_policy(
            app,
            0,
            "GPU Pocket CPU 深夜模式结束",
            "CPU 深夜模式已到北京时间 08:00 自动停止。",
        )
        .await;
        return Ok(());
    }

    let sample = read_cpu_usage().ok();
    let usage = {
        let mut runtime = app.cpu_runtime.lock().await;
        let usage = sample.as_ref().and_then(|current| {
            runtime.last_sample.and_then(|previous| {
                let total_delta = current.total.saturating_sub(previous.0);
                let idle_delta = current.idle.saturating_sub(previous.1);
                100u64
                    .saturating_mul(total_delta.saturating_sub(idle_delta))
                    .checked_div(total_delta)
                    .map(|percent| percent as u32)
            })
        });
        runtime.last_sample = sample.map(|sample| (sample.total, sample.idle));
        runtime.usage_percent = usage;
        if service == "inactive"
            && policy.idle_start_enabled
            && usage.is_some_and(|usage| usage < policy.idle_threshold_percent)
        {
            let since = runtime.low_usage_since.get_or_insert_with(Instant::now);
            Some(since.elapsed().as_secs())
        } else {
            runtime.low_usage_since = None;
            None
        }
    };

    let required_seconds = u64::from(policy.idle_duration_minutes) * 60;
    if policy.idle_start_enabled
        && service == "inactive"
        && usage.is_some()
        && app
            .cpu_runtime
            .lock()
            .await
            .low_usage_since
            .is_some_and(|since| since.elapsed().as_secs() >= required_seconds)
    {
        auto_cpu_action(app, Operation::Start, CpuMode::IdleAuto, false).await?;
        notify_policy(
            app,
            0,
            "GPU Pocket CPU 空闲自启",
            &format!(
                "整机 CPU 使用率低于 {}% 已持续 {} 分钟，CPU 矿工已启动。",
                policy.idle_threshold_percent, policy.idle_duration_minutes
            ),
        )
        .await;
    }
    Ok(())
}

async fn process_monitor_tick(app: &App) -> Result<()> {
    if app.demo {
        return Ok(());
    }
    let processes = gpu::processes(&app.config.gpu_root).await?;
    let mut current = BTreeMap::new();
    for process in processes
        .into_iter()
        .filter(|process| process.within_gpu_root != Some(true))
    {
        let location = process
            .executable
            .clone()
            .unwrap_or_else(|| format!("unknown:{}", process.name));
        current.insert((process.gpu_id, process.pid, location), process);
    }
    let newly_detected = {
        let mut watch = app.process_watch.lock().await;
        update_process_snapshot(&mut watch, current)
    };
    for gpu_id in GPU_IDS {
        let new_for_gpu: Vec<_> = newly_detected
            .iter()
            .filter(|process| process.gpu_id == gpu_id)
            .collect();
        if new_for_gpu.is_empty() {
            continue;
        }
        let listed = new_for_gpu
            .iter()
            .take(4)
            .map(|process| format!("{} (PID {})", process.name, process.pid))
            .collect::<Vec<_>>()
            .join("，");
        let remainder = new_for_gpu.len().saturating_sub(4);
        let suffix = if remainder > 0 {
            format!("，另有 {remainder} 个进程")
        } else {
            String::new()
        };
        notify_policy(
            app,
            gpu_id,
            "检测到 GPU 目录外进程",
            &format!("GPU{gpu_id} 新进程：{listed}{suffix}。请在 GPU Pocket 中查看进程路径。"),
        )
        .await;
    }
    Ok(())
}

fn update_process_snapshot(
    watch: &mut ProcessWatchState,
    current: BTreeMap<(u32, u32, String), gpu::GpuProcess>,
) -> Vec<gpu::GpuProcess> {
    let newly_detected = if watch.initialized {
        current
            .iter()
            .filter(|(key, _)| !watch.external.contains_key(*key))
            .map(|(_, process)| process.clone())
            .collect()
    } else {
        Vec::new()
    };
    watch.external = current;
    watch.initialized = true;
    newly_detected
}

fn is_china_night(now: SystemTime) -> Result<bool> {
    let local_seconds = now.duration_since(UNIX_EPOCH)?.as_secs() + 8 * 60 * 60;
    let hour = (local_seconds % 86_400) / 3_600;
    Ok((1..8).contains(&hour))
}

async fn notify_policy(app: &App, gpu_id: u32, title: &str, body: &str) {
    match app.push.send(title, body).await {
        Ok(()) => tracing::info!(gpu_id, "push notification sent"),
        Err(error) => tracing::error!(gpu_id, %error, "push notification delivery failed"),
    }
}

async fn auto_action(app: &App, gpu_id: u32, operation: Operation, mode: GpuMode) -> Result<()> {
    auto_action_with_manual_night_exit(app, gpu_id, operation, mode, false).await
}

async fn auto_action_end_manual_night(app: &App, gpu_id: u32) -> Result<()> {
    auto_action_with_manual_night_exit(app, gpu_id, Operation::Stop, GpuMode::ManualNight, true)
        .await
}

async fn auto_action_with_manual_night_exit(
    app: &App,
    gpu_id: u32,
    operation: Operation,
    mode: GpuMode,
    exit_manual_night: bool,
) -> Result<()> {
    let selection = read_desired(app, gpu_id)?.context("no staged GPU selection")?;
    let action = Action {
        operation,
        revision: selection.revision,
        mode: Some(mode),
    };
    let _ = action_for_gpu(app.clone(), gpu_id, action, mode, false, exit_manual_night)
        .await
        .map_err(|error| anyhow::anyhow!(error.1))?;
    Ok(())
}

async fn policy_tick(app: &App) -> Result<()> {
    cpu_policy_tick(app, SystemTime::now()).await?;
    if !app.config.control_enabled {
        let mut manual_night_active = false;
        for gpu_id in GPU_IDS {
            manual_night_active |= app.config.read_gpu_mode(gpu_id)? == GpuMode::ManualNight;
        }
        if !manual_night_active {
            return Ok(());
        }
    }
    let gpus = match gpu::collect().await {
        Ok(gpus) => gpus,
        Err(error) => {
            // A failed nvidia-smi sample is also unknown memory, not permission
            // to leave a manual night miner running without its memory guard.
            policy_tick_with_gpus(app, &[]).await?;
            return Err(error);
        }
    };
    policy_tick_with_gpus(app, &gpus).await
}

async fn policy_tick_with_gpus(app: &App, gpus: &[gpu::Gpu]) -> Result<()> {
    policy_tick_with_process_check(app, gpus, |gpu_id| async move {
        if app.demo {
            Ok(Vec::new())
        } else {
            gpu::unmanaged_compute_pids(gpu_id).await
        }
    })
    .await
}

async fn policy_tick_with_process_check<F, Fut>(
    app: &App,
    gpus: &[gpu::Gpu],
    check_processes: F,
) -> Result<()>
where
    F: Fn(u32) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u32>>>,
{
    policy_tick_with_process_check_at(app, gpus, SystemTime::now(), check_processes).await
}

async fn policy_tick_with_process_check_at<F, Fut>(
    app: &App,
    gpus: &[gpu::Gpu],
    now: SystemTime,
    check_processes: F,
) -> Result<()>
where
    F: Fn(u32) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u32>>>,
{
    for gpu_id in GPU_IDS {
        let policy = app.config.read_gpu_policy(gpu_id)?;
        let service = service_state(app, gpu_id).await?;
        let current_mode = app.config.read_gpu_mode(gpu_id)?;
        let runtime_lock = app.runtime.clone();
        if current_mode == GpuMode::ManualNight {
            if !is_china_night(now)? {
                auto_action_end_manual_night(app, gpu_id).await?;
                {
                    let mut runtime = runtime_lock.lock().await;
                    let state = runtime.entry(gpu_id).or_default();
                    state.low_util_since = None;
                    state.external_task = false;
                    state.memory_blocked = false;
                    state.last_message = None;
                }
                notify_policy(
                    app,
                    gpu_id,
                    "GPU Pocket 手动深夜模式结束",
                    &format!("GPU{gpu_id} 手动深夜模式已到北京时间 08:00 自动停止。"),
                )
                .await;
                continue;
            }
            let free_memory = gpus
                .iter()
                .find(|gpu| gpu.index == gpu_id)
                .and_then(free_gpu_memory_mib);
            let blocked = free_memory.is_none_or(|free| free < policy.min_free_memory_mib as f64);
            {
                let mut runtime = runtime_lock.lock().await;
                let state = runtime.entry(gpu_id).or_default();
                state.low_util_since = None;
                state.external_task = false;
                state.memory_blocked = blocked;
                state.last_message = blocked.then(|| {
                    format!(
                        "手动深夜模式显存保护：可用显存低于 {} MiB 或无法采样，需要手动重新启动",
                        policy.min_free_memory_mib
                    )
                });
            }
            if service == "active" && blocked {
                auto_action(app, gpu_id, Operation::Stop, GpuMode::ManualNight).await?;
                notify_policy(app, gpu_id, "GPU Pocket 手动深夜显存保护", &format!("GPU{gpu_id} 可用显存不足或无法采样，已停止手动深夜矿工；恢复后需手动重新启动。")).await;
            }
            continue;
        }
        if !app.config.control_enabled {
            continue;
        }
        if !policy.enabled {
            if service == "active" && is_automatic_mode(current_mode) {
                if let Err(error) = auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await
                {
                    tracing::error!(gpu_id, %error, "failed to stop automation after policy disable");
                }
            }
            let mut runtime = runtime_lock.lock().await;
            runtime.remove(&gpu_id);
            continue;
        }

        let Some(metrics) = gpus.iter().find(|gpu| gpu.index == gpu_id) else {
            runtime_lock
                .lock()
                .await
                .entry(gpu_id)
                .or_default()
                .low_util_since = None;
            continue;
        };
        let night = policy.night_mode_enabled && is_china_night(now)?;
        let monitor_external = policy.external_task_monitor
            || policy.throttle_enabled
            || policy.night_mode_enabled
            || is_automatic_mode(current_mode);
        let unmanaged_result = if !monitor_external {
            Ok(Vec::new())
        } else {
            check_processes(gpu_id).await
        };
        let unmanaged = match unmanaged_result {
            Ok(pids) => pids,
            Err(error) => {
                {
                    let mut runtime = runtime_lock.lock().await;
                    let state = runtime.entry(gpu_id).or_default();
                    state.low_util_since = None;
                    state.last_message = Some(format!("进程归属检查失败：{error}"));
                }
                if service == "active"
                    && should_stop_for_external_task(current_mode, policy.external_task_monitor)
                {
                    auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await?;
                    notify_policy(
                        app,
                        gpu_id,
                        "GPU Pocket 安全暂停",
                        &format!("GPU{gpu_id} 无法确认计算进程归属，已停止托管矿工。"),
                    )
                    .await;
                }
                continue;
            }
        };
        let external_task = !unmanaged.is_empty();
        let was_external = {
            let mut runtime = runtime_lock.lock().await;
            let state = runtime.entry(gpu_id).or_default();
            let previous = state.external_task;
            state.external_task = external_task;
            if external_task {
                state.low_util_since = None;
                state.last_message = Some(format!("检测到 GPU{gpu_id} 外部进程：{unmanaged:?}"));
            } else if previous
                || state
                    .last_message
                    .as_deref()
                    .is_some_and(|message| message.starts_with("进程归属检查失败："))
            {
                state.last_message = None;
            }
            previous
        };
        if external_task {
            if service == "active" && current_mode == GpuMode::ManualThrottle {
                let free_memory = metrics
                    .memory_total_mib
                    .zip(metrics.memory_used_mib)
                    .map(|(total, used)| (total - used).max(0.0));
                if free_memory.is_none_or(|free| free < policy.min_free_memory_mib as f64) {
                    if let Err(error) =
                        auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await
                    {
                        tracing::error!(gpu_id, %error, "failed to stop manual throttle miner for low memory");
                    } else {
                        notify_policy(
                            app,
                            gpu_id,
                            "GPU Pocket 显存保护",
                            &format!(
                                "GPU{gpu_id} 可用显存低于 {} MiB，已停止手动节制矿工。",
                                policy.min_free_memory_mib
                            ),
                        )
                        .await;
                    }
                }
            }
            if service == "active"
                && should_stop_for_external_task(current_mode, policy.external_task_monitor)
            {
                if let Err(error) = auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await
                {
                    tracing::error!(gpu_id, %error, "failed to stop miner for external GPU task");
                }
            }
            if !was_external && policy.external_task_monitor {
                notify_policy(
                    app,
                    gpu_id,
                    "GPU Pocket 已暂停挖矿",
                    &format!("GPU{gpu_id} 检测到其他计算进程，已停止 GPU Pocket 管理的矿工。"),
                )
                .await;
            }
            continue;
        }

        if current_mode == GpuMode::ManualThrottle {
            runtime_lock
                .lock()
                .await
                .entry(gpu_id)
                .or_default()
                .low_util_since = None;
            let free_memory = metrics
                .memory_total_mib
                .zip(metrics.memory_used_mib)
                .map(|(total, used)| (total - used).max(0.0));
            let memory_is_low =
                free_memory.is_none_or(|free| free < policy.min_free_memory_mib as f64);
            if service == "active" && memory_is_low {
                if let Err(error) = auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await
                {
                    tracing::error!(gpu_id, %error, "failed to stop manual throttle miner for low memory");
                } else {
                    notify_policy(
                        app,
                        gpu_id,
                        "GPU Pocket 显存保护",
                        &format!(
                            "GPU{gpu_id} 可用显存低于 {} MiB，已停止手动节制矿工。",
                            policy.min_free_memory_mib
                        ),
                    )
                    .await;
                }
            }
            continue;
        }

        let free_memory = metrics
            .memory_total_mib
            .zip(metrics.memory_used_mib)
            .map(|(total, used)| (total - used).max(0.0));
        let low_util_seconds = {
            let mut runtime = runtime_lock.lock().await;
            let state = runtime.entry(gpu_id).or_default();
            if should_track_auto_start_idle(
                policy.throttle_enabled,
                metrics.utilization,
                service == "active",
            ) {
                let since = state.low_util_since.get_or_insert_with(Instant::now);
                since.elapsed().as_secs()
            } else {
                state.low_util_since = None;
                0
            }
        };
        let automation_due =
            night || (policy.throttle_enabled && low_util_seconds >= AUTO_START_IDLE_SECONDS);
        let memory_guard_active = current_mode != GpuMode::Normal || automation_due;
        let mut memory_blocked = false;
        let mut memory_blocked_now = false;
        {
            let mut runtime = runtime_lock.lock().await;
            let state = runtime.entry(gpu_id).or_default();
            if (memory_guard_active || state.memory_blocked)
                && free_memory.is_none_or(|free| free < policy.min_free_memory_mib as f64)
            {
                memory_blocked = true;
                if !state.memory_blocked {
                    memory_blocked_now = true;
                    state.last_message = Some(format!(
                        "可用显存低于 {} MiB，自动停止保护已触发",
                        policy.min_free_memory_mib
                    ));
                    state.memory_blocked = true;
                }
            } else if free_memory
                .is_some_and(|free| free >= (policy.min_free_memory_mib + 512) as f64)
            {
                state.memory_blocked = false;
                if state
                    .last_message
                    .as_deref()
                    .is_some_and(|message| message.starts_with("可用显存低于 "))
                {
                    state.last_message = None;
                }
            }
        }
        if memory_blocked {
            if service == "active" && current_mode != GpuMode::Normal {
                auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await?;
            }
            if memory_blocked_now {
                notify_policy(
                    app,
                    gpu_id,
                    "GPU Pocket 显存保护",
                    &format!(
                        "GPU{gpu_id} 可用显存低于 {} MiB，自动模式已暂停。",
                        policy.min_free_memory_mib
                    ),
                )
                .await;
            }
            continue;
        }

        if night {
            if service != "active" || current_mode != GpuMode::Night {
                auto_action(app, gpu_id, Operation::Restart, GpuMode::Night).await?;
                notify_policy(
                    app,
                    gpu_id,
                    "GPU Pocket 深夜模式",
                    &format!("GPU{gpu_id} 已按北京时间 01:00–08:00 进入显存保护模式。"),
                )
                .await;
            }
            continue;
        }
        if service == "active" && current_mode == GpuMode::Night {
            auto_action(app, gpu_id, Operation::Stop, GpuMode::Normal).await?;
            continue;
        }

        if should_auto_start(
            policy.throttle_enabled,
            low_util_seconds,
            service == "active",
        ) {
            auto_action(app, gpu_id, Operation::Start, GpuMode::Normal).await?;
            notify_policy(
                app,
                gpu_id,
                "GPU Pocket 自启模式",
                &format!("GPU{gpu_id} 利用率低于 {AUTO_START_UTILIZATION_THRESHOLD}% 已持续 10 分钟，已按全量强度 31 启动。"),
            )
            .await;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    operation: Operation,
    revision: u64,
    #[serde(default)]
    mode: Option<GpuMode>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Operation {
    Start,
    Restart,
    Stop,
}

async fn action(State(app): State<App>, Json(action): Json<Action>) -> ApiResult {
    action_for_gpu(
        app,
        0,
        action,
        action.mode.unwrap_or(GpuMode::Normal),
        true,
        false,
    )
    .await
}

async fn action_gpu(
    State(app): State<App>,
    axum::extract::Path(gpu_id): axum::extract::Path<u32>,
    Json(action): Json<Action>,
) -> ApiResult {
    action_for_gpu(
        app,
        gpu_id,
        action,
        action.mode.unwrap_or(GpuMode::Normal),
        true,
        false,
    )
    .await
}

async fn action_for_gpu(
    app: App,
    gpu_id: u32,
    action: Action,
    mode: GpuMode,
    manual: bool,
    exit_manual_night: bool,
) -> ApiResult {
    if manual {
        writable(&app)?;
    }
    let unit = unit_name(gpu_id).map_err(bad)?;
    let _guard = app.mutation.lock().await;
    let current_mode = app.config.read_gpu_mode(gpu_id)?;
    // Re-check after taking the mutation lock: a policy decision may have been
    // queued before the user took manual control. Only a tagged memory stop is
    // allowed to act automatically on a manual night session.
    if !manual
        && current_mode == GpuMode::ManualNight
        && !exit_manual_night
        && !(action.operation == Operation::Stop && mode == GpuMode::ManualNight)
    {
        tracing::info!(
            gpu_id,
            "skipped stale automation during manual night session"
        );
        return Ok(Json(
            json!({"ok":true,"gpu_id":gpu_id,"mode":current_mode,"skipped":true}),
        ));
    }
    let selection = read_desired(&app, gpu_id)?;
    if selection.as_ref().map_or(0, |s| s.revision) != action.revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "selection changed; refresh before applying".into(),
        ));
    }
    let requested_mode = if action.operation == Operation::Stop {
        GpuMode::Normal
    } else {
        action.mode.unwrap_or(mode)
    };
    if manual && requested_mode == GpuMode::Throttle {
        return Err(ApiError(
            StatusCode::GONE,
            "manual throttle mode is disabled: SRBMiner intensity does not cap GPU utilization"
                .into(),
        ));
    }
    if requested_mode == GpuMode::Throttle {
        let selected = selection
            .as_ref()
            .ok_or_else(|| bad("save a GPU selection first"))?;
        let (profile, _) = app.config.resolve(selected).map_err(bad)?;
        if profile.miner != "srb-pearl" {
            return Err(bad(
                "throttle intensity is only supported by SRBMiner Pearl",
            ));
        }
        let policy = app.config.read_gpu_policy(gpu_id)?;
        if !(1..=31).contains(&policy.intensity) {
            return Err(bad("SRBMiner throttle intensity must be between 1 and 31"));
        }
    }
    let operation = match action.operation {
        Operation::Start => "start",
        Operation::Restart => "restart",
        Operation::Stop => "stop",
    };
    let active = if operation == "stop" {
        "inactive".to_string()
    } else {
        service_state(&app, gpu_id).await?
    };
    let mode_to_apply = mode_for_action(
        action.operation,
        &active,
        requested_mode,
        current_mode,
        manual,
        exit_manual_night,
    );
    if operation != "stop" {
        let s = selection
            .as_ref()
            .ok_or_else(|| bad("save a selection first"))?;
        let (profile, _) = app.config.resolve(s).map_err(bad)?;
        if requested_mode == GpuMode::ManualNight {
            if profile.miner != "srb-pearl" {
                return Err(bad("manual night mode requires SRBMiner Pearl"));
            }
            let policy = app.config.read_gpu_policy(gpu_id)?;
            let gpus = if app.demo {
                gpu::demo()
            } else {
                gpu::collect().await?
            };
            let free_memory = gpus
                .iter()
                .find(|gpu| gpu.index == gpu_id)
                .and_then(free_gpu_memory_mib);
            if free_memory.is_none_or(|free| free < policy.min_free_memory_mib as f64) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    format!(
                        "手动深夜模式需要至少 {} MiB 可用显存，当前不足或无法采样",
                        policy.min_free_memory_mib
                    ),
                ));
            }
        }
        if !app.demo {
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
            if requires_exclusive_gpu(mode_to_apply, manual) {
                gpu::ensure_gpu_owned(gpu_id)
                    .await
                    .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
            }
        }
    }
    app.config.save_gpu_mode(gpu_id, mode_to_apply)?;
    if app.demo {
        let mut active = app.demo_active.lock().await;
        if operation == "stop" {
            active.remove(&gpu_id);
        } else if operation == "restart" || !active.contains_key(&gpu_id) {
            let mut applied = selection.clone().unwrap();
            applied.gpu_intensity = None;
            applied.power_limit_w = None;
            app.config.save_gpu_selection("applied", &applied)?;
            active.insert(gpu_id, applied);
        }
    } else {
        // Automatic systemd recovery must reuse the last explicitly applied version.
        // Merely staging another wallet must never change a later crash restart.
        if operation != "stop"
            && (operation == "restart" || service_state(&app, gpu_id).await? != "active")
        {
            let mut applied = selection.clone().unwrap();
            applied.gpu_intensity = None;
            applied.power_limit_w = None;
            app.config.save_gpu_selection("applied", &applied)?;
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
        json!({"ok": true, "gpu_id":gpu_id,"operation":operation,"mode":mode_to_apply,"demo":app.demo}),
    ))
}

fn requires_exclusive_gpu(mode: GpuMode, manual: bool) -> bool {
    !manual || !matches!(mode, GpuMode::Normal | GpuMode::ManualNight)
}

fn free_gpu_memory_mib(gpu: &gpu::Gpu) -> Option<f64> {
    gpu.memory_total_mib
        .zip(gpu.memory_used_mib)
        .map(|(total, used)| (total - used).max(0.0))
}

fn is_automatic_mode(mode: GpuMode) -> bool {
    matches!(mode, GpuMode::Throttle | GpuMode::Night)
}

fn should_stop_for_external_task(mode: GpuMode, monitor_enabled: bool) -> bool {
    is_automatic_mode(mode) || (mode == GpuMode::Normal && monitor_enabled)
}

fn should_auto_start(enabled: bool, low_util_seconds: u64, service_active: bool) -> bool {
    enabled && low_util_seconds >= AUTO_START_IDLE_SECONDS && !service_active
}

fn should_track_auto_start_idle(
    enabled: bool,
    utilization: Option<f64>,
    service_active: bool,
) -> bool {
    enabled
        && !service_active
        && utilization.is_some_and(|value| value < AUTO_START_UTILIZATION_THRESHOLD)
}

fn mode_for_action(
    operation: Operation,
    service: &str,
    requested_mode: GpuMode,
    current_mode: GpuMode,
    manual: bool,
    exit_manual_night: bool,
) -> GpuMode {
    if operation == Operation::Stop {
        if !manual && current_mode == GpuMode::ManualNight && !exit_manual_night {
            GpuMode::ManualNight
        } else {
            GpuMode::Normal
        }
    } else if operation == Operation::Restart || service != "active" {
        if manual && requested_mode == GpuMode::Throttle {
            GpuMode::ManualThrottle
        } else {
            requested_mode
        }
    } else {
        current_mode
    }
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

fn ensure_miner_binary(config: &Config, relative: &std::path::Path) -> Result<std::path::PathBuf> {
    let root = config.gpu_root.canonicalize()?;
    let binary = config.gpu_root.join(relative).canonicalize()?;
    if !binary.starts_with(root) {
        bail!("binary escaped gpu_root");
    }
    Ok(binary)
}

fn srbminer_intensity(mode: GpuMode, throttle_intensity: u32) -> Result<u32> {
    let intensity = if matches!(mode, GpuMode::Throttle | GpuMode::ManualThrottle) {
        throttle_intensity
    } else {
        31
    };
    if !(1..=31).contains(&intensity) {
        bail!("SRBMiner GPU intensity must be between 1 and 31");
    }
    Ok(intensity)
}

fn launch_cpu(config: &Config) -> Result<()> {
    let selection = config.read_cpu_selection("applied")?;
    let (profile, pool) = config.resolve_cpu(&selection)?;
    let binary = ensure_miner_binary(config, &profile.binary)?;
    let mut command = std::process::Command::new(&binary);
    command.current_dir(binary.parent().context("binary has no parent")?);
    command.args([
        "--algorithm-cpu",
        &profile.algorithm,
        "--disable-gpu",
        "--pool",
        &pool.address,
        "--wallet",
        &selection.wallet,
        "--pass",
        "x",
        "--tls",
        if pool.tls { "true" } else { "false" },
        "--cpu-threads-percent",
        &selection.threads_percent.to_string(),
        "--retry-time",
        "5",
        "--give-up-limit",
        "3",
    ]);
    config.save_cpu_selection("active", &selection)?;
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

fn launch(config: &Config, gpu_id: u32) -> Result<()> {
    unit_name(gpu_id)?;
    let selection = config.read_gpu_selection("applied", gpu_id)?;
    if selection.gpu_id != gpu_id {
        bail!("applied selection does not match the miner GPU");
    }
    let (profile, pool) = config.resolve(&selection)?;
    let mode = config.read_gpu_mode(gpu_id)?;
    let policy = config.read_gpu_policy(gpu_id)?;
    let binary = ensure_miner_binary(config, &profile.binary)?;
    let mut command = std::process::Command::new(&binary);
    command.current_dir(binary.parent().context("binary has no parent")?);
    let tls = if pool.tls { "true" } else { "false" };
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
        let intensity = srbminer_intensity(mode, policy.intensity)?;
        command.args(["--gpu-intensity", &intensity.to_string()]);
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
        if mode != GpuMode::Normal {
            command.args(["--keepfree", &policy.min_free_memory_mib.to_string()]);
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
    let cpu_miner = args.get(1).is_some_and(|s| s == "cpu-miner");
    let miner = cpu_miner || args.get(1).is_some_and(|s| s == "miner");
    let path = args
        .get(if miner { 2 } else { 1 })
        .map(String::as_str)
        .unwrap_or("server.toml");
    let config = Config::load(path)?;
    if miner {
        if cpu_miner {
            return launch_cpu(&config);
        }
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
    let mut push_client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15));
    if let Ok(proxy) = std::env::var("GPU_POCKET_FCM_PROXY") {
        if !proxy.trim().is_empty() {
            push_client = push_client.proxy(reqwest::Proxy::all(proxy.trim())?);
        }
    }
    let push = Arc::new(push::PushService::new(
        config.state_dir.clone(),
        push_client.build()?,
    ));
    let app = App {
        ai: Arc::new(Mutex::new(ai::AiState::load(&config.state_dir)?)),
        config: Arc::new(config),
        token: Arc::new(token),
        demo,
        mutation: Arc::new(Mutex::new(())),
        demo_active: Arc::new(Mutex::new(BTreeMap::new())),
        demo_cpu_active: Arc::new(Mutex::new(None)),
        demo_node: Arc::new(Mutex::new("DEMO-HK".into())),
        runtime: Arc::new(Mutex::new(BTreeMap::new())),
        process_watch: Arc::new(Mutex::new(ProcessWatchState::default())),
        cpu_usage: Arc::new(Mutex::new(None)),
        cpu_runtime: Arc::new(Mutex::new(CpuRuntime::default())),
        push,
        client,
    };
    if !demo {
        tokio::spawn(policy_loop(app.clone()));
        tokio::spawn(process_monitor_loop(app.clone()));
    }
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
