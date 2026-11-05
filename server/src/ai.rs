//! Authenticated task relay. Only the desktop worker runs an AI CLI.
use super::{bad, ApiError, ApiResult, App};
use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::Path as FilePath,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const HISTORY_FILE: &str = "ai-tasks.json";
const WORKSPACE: &str = r"D:\work\github\gpu-pocket";
const LEASE: Duration = Duration::from_secs(60);

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    id: String,
    available: bool,
    message: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Task {
    id: String,
    provider: String,
    prompt: String,
    allow_edits: bool,
    status: String,
    output: String,
    created_at: u64,
    worker_id: Option<String>,
}

struct Worker {
    id: String,
    seen: Instant,
    providers: Vec<Provider>,
}

#[derive(Default)]
pub struct AiState {
    tasks: Vec<Task>,
    worker: Option<Worker>,
}

impl AiState {
    pub fn load(directory: &FilePath) -> Result<Self> {
        let path = directory.join(HISTORY_FILE);
        let mut tasks: Vec<Task> = if path.exists() {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            Vec::new()
        };
        for task in &mut tasks {
            if matches!(task.status.as_str(), "running" | "cancelling") {
                task.status = "failed".into();
                task.output = "API 已重启，任务被中断；请重新提交。".into();
            }
        }
        Ok(Self {
            tasks,
            worker: None,
        })
    }

    fn online(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| worker.seen.elapsed() < LEASE)
    }

    fn expire(&mut self) {
        if !self.online() {
            for task in &mut self.tasks {
                if matches!(task.status.as_str(), "running" | "cancelling") {
                    task.status = "failed".into();
                    task.output = "本机 AI 连接中断，任务已终止；请检查电脑后重新提交。".into();
                }
            }
        }
    }

    fn save(&self, app: &App) -> Result<()> {
        app.config.save_json(HISTORY_FILE, &self.tasks)
    }
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/ai", get(status))
        .route("/api/v1/ai/tasks", post(submit))
        .route("/api/v1/ai/tasks/{id}/cancel", post(cancel))
        .route("/api/v1/ai/worker/poll", post(poll))
        .route("/api/v1/ai/worker/result", post(result))
        .layer(DefaultBodyLimit::max(128 * 1024))
}

async fn status(State(app): State<App>) -> ApiResult {
    let mut state = app.ai.lock().await;
    state.expire();
    let providers: Vec<_> = ["codex", "cursor"].into_iter().map(|id| {
        let provider = state.worker.as_ref().and_then(|worker| worker.providers.iter().find(|p| p.id == id));
        json!({"id":id,"available":state.online() && provider.is_some_and(|p| p.available),
            "message":if state.online() { provider.map(|p| p.message.as_str()).unwrap_or("尚未配置") } else { "本机 AI 工作进程未连接" }})
    }).collect();
    Ok(Json(
        json!({"workspace":WORKSPACE,"providers":providers,"tasks":state.tasks.iter().rev().collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    provider: String,
    prompt: String,
    #[serde(default)]
    allow_edits: bool,
}

async fn submit(State(app): State<App>, Json(input): Json<Submission>) -> ApiResult {
    if !["codex", "cursor"].contains(&input.provider.as_str())
        || input.prompt.trim().is_empty()
        || input.prompt.len() > 8000
    {
        return Err(bad(
            "请选择 Codex 或 Cursor，并填写最多 8000 字节的任务描述。",
        ));
    }
    let mut state = app.ai.lock().await;
    state.expire();
    if !state.online()
        || !state.worker.as_ref().is_some_and(|w| {
            w.providers
                .iter()
                .any(|p| p.id == input.provider && p.available)
        })
    {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "所选本机 AI 尚未就绪，请检查电脑上的登录状态。".into(),
        ));
    }
    if state
        .tasks
        .iter()
        .filter(|task| matches!(task.status.as_str(), "queued" | "running" | "cancelling"))
        .count()
        >= 4
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "任务队列已满，请等待或取消已有任务。".into(),
        ));
    }
    if state.tasks.len() >= 20 {
        if let Some(index) = state
            .tasks
            .iter()
            .position(|t| !matches!(t.status.as_str(), "queued" | "running" | "cancelling"))
        {
            state.tasks.remove(index);
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let task = Task {
        id: now.as_nanos().to_string(),
        provider: input.provider,
        prompt: input.prompt.trim().into(),
        allow_edits: input.allow_edits,
        status: "queued".into(),
        output: String::new(),
        created_at: now.as_secs(),
        worker_id: None,
    };
    state.tasks.push(task.clone());
    state.save(&app)?;
    Ok(Json(json!({"task":task})))
}

async fn cancel(State(app): State<App>, Path(id): Path<String>) -> ApiResult {
    let mut state = app.ai.lock().await;
    let task = state
        .tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| bad("任务不存在"))?;
    match task.status.as_str() {
        "queued" => task.status = "cancelled".into(),
        "running" => task.status = "cancelling".into(),
        _ => (),
    }
    state.save(&app)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Poll {
    worker_id: String,
    providers: Vec<Provider>,
    running_task_id: Option<String>,
}

async fn poll(State(app): State<App>, Json(input): Json<Poll>) -> ApiResult {
    if input.worker_id.is_empty()
        || input.worker_id.len() > 100
        || input.providers.len() > 2
        || input
            .providers
            .iter()
            .any(|p| !["codex", "cursor"].contains(&p.id.as_str()) || p.message.len() > 300)
    {
        return Err(bad("invalid worker capabilities"));
    }
    let mut state = app.ai.lock().await;
    state.expire();
    if state.online()
        && state
            .worker
            .as_ref()
            .is_some_and(|w| w.id != input.worker_id)
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "另一台电脑已连接 AI 任务队列".into(),
        ));
    }
    state.worker = Some(Worker {
        id: input.worker_id.clone(),
        seen: Instant::now(),
        providers: input.providers.clone(),
    });
    if let Some(id) = &input.running_task_id {
        let cancel = !state.tasks.iter().any(|t| {
            &t.id == id && t.worker_id.as_ref() == Some(&input.worker_id) && t.status == "running"
        });
        return Ok(Json(json!({"task":null,"cancel":cancel})));
    }
    // Re-deliver an unacknowledged claim after a lost HTTP response.
    if let Some(task) = state.tasks.iter().find(|t| {
        t.worker_id.as_ref() == Some(&input.worker_id)
            && matches!(t.status.as_str(), "running" | "cancelling")
    }) {
        return Ok(Json(
            json!({"task":task,"cancel":task.status == "cancelling"}),
        ));
    }
    if let Some(task) = state.tasks.iter_mut().find(|t| {
        t.status == "queued"
            && input
                .providers
                .iter()
                .any(|p| p.id == t.provider && p.available)
    }) {
        task.status = "running".into();
        task.worker_id = Some(input.worker_id);
        let task = task.clone();
        state.save(&app)?;
        return Ok(Json(json!({"task":task,"cancel":false})));
    }
    Ok(Json(json!({"task":null,"cancel":false})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    worker_id: String,
    id: String,
    status: String,
    output: String,
}

async fn result(State(app): State<App>, Json(input): Json<Completion>) -> ApiResult {
    if !["completed", "failed", "cancelled"].contains(&input.status.as_str())
        || input.output.len() > 24000
    {
        return Err(bad("invalid task result"));
    }
    let mut state = app.ai.lock().await;
    let task = state
        .tasks
        .iter_mut()
        .find(|t| t.id == input.id && t.worker_id.as_ref() == Some(&input.worker_id))
        .ok_or_else(|| bad("任务归属不匹配"))?;
    if matches!(task.status.as_str(), "running" | "cancelling") {
        task.status = if task.status == "cancelling" {
            "cancelled".into()
        } else {
            input.status
        };
        task.output = input.output;
    }
    state.save(&app)?;
    Ok(Json(json!({"ok":true})))
}
