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
use config::{Config, GpuMode, GpuPolicy, Selection};
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

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    token: Arc<String>,
    demo: bool,
    mutation: Arc<Mutex<()>>,
    demo_active: Arc<Mutex<BTreeMap<u32, Selection>>>,
    demo_node: Arc<Mutex<String>>,
    runtime: Arc<Mutex<BTreeMap<u32, RuntimeState>>>,
    push: Arc<push::PushService>,
    client: reqwest::Client,
}

#[derive(Default)]
struct RuntimeState {
    low_util_since: Option<Instant>,
    external_task: bool,
    memory_blocked: bool,
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
        .route("/api/v1/status", get(status))
        .route("/api/v1/catalog", get(catalog))
        .route("/api/v1/selection", put(save))
        .route("/api/v1/miner", post(action))
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
    let mut policies = BTreeMap::new();
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
        gpu_controls.insert(
            gpu_id.to_string(),
            json!({"service":service,"desired":desired,"active":active}),
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
    Ok(Json(
        json!({"host": "2-titan", "demo": app.demo, "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        "gpus": gpus, "service": service, "desired": desired, "active": active,
        "gpu_controls":gpu_controls,"policies":policies,
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
    let (profile, _) = app.config.resolve(&selected).map_err(bad)?;
    let policy = app.config.read_gpu_policy(gpu_id)?;
    if policy.enabled
        && (policy.throttle_enabled || policy.night_mode_enabled)
        && profile.miner != "srb-pearl"
    {
        return Err(bad(
            "disable SRBMiner automation before selecting a different miner",
        ));
    }
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
    if !(512..=16_384).contains(&policy.min_free_memory_mib) {
        return Err(bad("min_free_memory_mib must be between 512 and 16384"));
    }
    if policy.intensity > 31 {
        return Err(bad("SRBMiner intensity must be between 0 and 31"));
    }
    if policy.throttle_enabled || policy.night_mode_enabled {
        let selection = read_desired(&app, gpu_id)?
            .ok_or_else(|| bad("save a GPU selection before enabling automation"))?;
        let (profile, _) = app.config.resolve(&selection).map_err(bad)?;
        if profile.miner != "srb-pearl" {
            return Err(bad(
                "automatic throttle and night modes currently require SRBMiner Pearl",
            ));
        }
    }
    if let Some(power_limit_w) = policy.power_limit_w {
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
    let selection = read_desired(app, gpu_id)?.context("no staged GPU selection")?;
    let action = Action {
        operation,
        revision: selection.revision,
    };
    let _ = action_for_gpu(app.clone(), gpu_id, action, mode)
        .await
        .map_err(|error| anyhow::anyhow!(error.1))?;
    Ok(())
}

async fn policy_tick(app: &App) -> Result<()> {
    if !app.config.control_enabled {
        return Ok(());
    }
    let gpus = gpu::collect().await?;
    let now = SystemTime::now();
    for gpu_id in GPU_IDS {
        let policy = app.config.read_gpu_policy(gpu_id)?;
        let service = service_state(app, gpu_id).await?;
        let current_mode = app.config.read_gpu_mode(gpu_id)?;
        let runtime_lock = app.runtime.clone();
        if !policy.enabled {
            if service == "active" && current_mode != GpuMode::Normal {
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
            continue;
        };
        let night = policy.night_mode_enabled && is_china_night(now)?;
        let night_only_memory = night || current_mode == GpuMode::Night;
        let monitor_external = !night_only_memory
            && (current_mode == GpuMode::Throttle
                || (policy.enabled && (policy.external_task_monitor || policy.throttle_enabled)));
        let unmanaged_result = if app.demo || !monitor_external {
            Ok(Vec::new())
        } else {
            gpu::unmanaged_compute_pids(gpu_id).await
        };
        let unmanaged = match unmanaged_result {
            Ok(pids) => pids,
            Err(error) => {
                {
                    let mut runtime = runtime_lock.lock().await;
                    runtime.entry(gpu_id).or_default().last_message =
                        Some(format!("进程归属检查失败：{error}"));
                }
                if service == "active"
                    && (current_mode != GpuMode::Normal || policy.external_task_monitor)
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
            state.low_util_since = None;
            let previous = state.external_task;
            state.external_task = external_task;
            if external_task {
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
            if service == "active"
                && (current_mode != GpuMode::Normal || policy.external_task_monitor)
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

        let free_memory = metrics
            .memory_total_mib
            .zip(metrics.memory_used_mib)
            .map(|(total, used)| (total - used).max(0.0));
        let low_util_seconds = {
            let mut runtime = runtime_lock.lock().await;
            let state = runtime.entry(gpu_id).or_default();
            if policy.throttle_enabled && metrics.utilization.is_some_and(|value| value < 5.0) {
                let since = state.low_util_since.get_or_insert_with(Instant::now);
                since.elapsed().as_secs()
            } else {
                state.low_util_since = None;
                0
            }
        };
        let automation_due = night || (policy.throttle_enabled && low_util_seconds >= 30 * 60);
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

        if policy.throttle_enabled
            && low_util_seconds >= 30 * 60
            && (service != "active" || current_mode != GpuMode::Throttle)
        {
            let operation = if service == "active" {
                Operation::Restart
            } else {
                Operation::Start
            };
            auto_action(app, gpu_id, operation, GpuMode::Throttle).await?;
            notify_policy(
                app,
                gpu_id,
                "GPU Pocket 节制模式",
                &format!("GPU{gpu_id} 利用率低于 5% 已持续 30 分钟，已启动节制挖矿。"),
            )
            .await;
        }
    }
    Ok(())
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
    action_for_gpu(app, 0, action, GpuMode::Normal).await
}

async fn action_gpu(
    State(app): State<App>,
    axum::extract::Path(gpu_id): axum::extract::Path<u32>,
    Json(action): Json<Action>,
) -> ApiResult {
    action_for_gpu(app, gpu_id, action, GpuMode::Normal).await
}

async fn action_for_gpu(app: App, gpu_id: u32, action: Action, mode: GpuMode) -> ApiResult {
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
    let mode_to_apply = if operation == "stop" {
        GpuMode::Normal
    } else if operation == "restart" || service_state(&app, gpu_id).await? != "active" {
        mode
    } else {
        app.config.read_gpu_mode(gpu_id)?
    };
    app.config.save_gpu_mode(gpu_id, mode_to_apply)?;
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
    let mode = config.read_gpu_mode(gpu_id)?;
    let policy = config.read_gpu_policy(gpu_id)?;
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
        let power_limit_w = match mode {
            GpuMode::Normal => selection.power_limit_w,
            GpuMode::Throttle => policy.power_limit_w,
            GpuMode::Night => None,
        };
        if let Some(power_limit_w) = power_limit_w {
            let power_argument = format!("--gpu-plimit{gpu_id}");
            let power_value = power_limit_w.to_string();
            command.args([power_argument.as_str(), power_value.as_str()]);
        }
        if mode == GpuMode::Throttle {
            command.args(["--gpu-intensity", &policy.intensity.to_string()]);
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
        if mode != GpuMode::Normal {
            command.args(["--keepfree", &policy.min_free_memory_mib.to_string()]);
        }
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
    let push = Arc::new(push::PushService::new(
        config.state_dir.clone(),
        client.clone(),
    ));
    let app = App {
        config: Arc::new(config),
        token: Arc::new(token),
        demo,
        mutation: Arc::new(Mutex::new(())),
        demo_active: Arc::new(Mutex::new(BTreeMap::new())),
        demo_node: Arc::new(Mutex::new("DEMO-HK".into())),
        runtime: Arc::new(Mutex::new(BTreeMap::new())),
        push,
        client,
    };
    if !demo {
        tokio::spawn(policy_loop(app.clone()));
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
