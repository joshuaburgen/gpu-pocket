use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use std::collections::BTreeMap;
use tower::ServiceExt;

const TOKEN: &str = "test-only-32-characters-abcdefghijk";

#[tokio::test]
async fn ai_tasks_require_online_provider_and_support_claim_cancel_and_persistence() {
    let (app, dir) = fixture();
    let submission = json!({"provider":"codex","prompt":"Explain GPU Pocket","allow_edits":false});
    assert_eq!(
        call(&app, "POST", "/api/v1/ai/tasks", submission.clone())
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let worker = json!({"worker_id":"desktop","providers":[{"id":"codex","available":true,"message":"ready"},{"id":"cursor","available":false,"message":"login required"}],"running_task_id":null});
    assert_eq!(
        call(&app, "POST", "/api/v1/ai/worker/poll", worker.clone())
            .await
            .0,
        StatusCode::OK
    );
    let (code, created) = call(&app, "POST", "/api/v1/ai/tasks", submission.clone()).await;
    assert_eq!(code, StatusCode::OK);
    let id = created["task"]["id"].as_str().unwrap();
    let (_, claim) = call(&app, "POST", "/api/v1/ai/worker/poll", worker.clone()).await;
    assert_eq!(claim["task"]["id"], id);
    assert_eq!(claim["task"]["allow_edits"], false);
    let (_, retry) = call(&app, "POST", "/api/v1/ai/worker/poll", worker.clone()).await;
    assert_eq!(retry["task"]["id"], id);
    let mut heartbeat = worker.clone();
    heartbeat["running_task_id"] = json!(id);
    let (_, running) = call(&app, "POST", "/api/v1/ai/worker/poll", heartbeat.clone()).await;
    assert!(running["task"].is_null());
    assert_eq!(running["cancel"], false);
    let mut other = worker.clone();
    other["worker_id"] = json!("other");
    assert_eq!(
        call(&app, "POST", "/api/v1/ai/worker/poll", other).await.0,
        StatusCode::CONFLICT
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/ai/tasks/{id}/cancel"),
        json!({}),
    )
    .await;
    let (_, cancelling) = call(&app, "POST", "/api/v1/ai/worker/poll", heartbeat).await;
    assert_eq!(cancelling["cancel"], true);
    let result = json!({"worker_id":"desktop","id":id,"status":"completed","output":"result"});
    assert_eq!(
        call(&app, "POST", "/api/v1/ai/worker/result", result.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/ai/worker/result", result)
            .await
            .0,
        StatusCode::OK
    );
    *app.ai.lock().await = ai::AiState::load(dir.path()).unwrap();
    let (_, status) = call(&app, "GET", "/api/v1/ai", Value::Null).await;
    assert_eq!(status["tasks"][0]["status"], "cancelled");
    assert_eq!(status["tasks"][0]["output"], "result");
    assert_eq!(status["providers"][0]["available"], false);
}

#[tokio::test]
async fn ai_queue_rejects_workspace_injection_and_unavailable_cursor() {
    let (app, _dir) = fixture();
    let worker = json!({"worker_id":"desktop","providers":[{"id":"codex","available":true,"message":"ready"}],"running_task_id":null});
    call(&app, "POST", "/api/v1/ai/worker/poll", worker).await;
    let injection = json!({"provider":"codex","prompt":"test","workspace":"C:/Windows"});
    assert_eq!(
        call(&app, "POST", "/api/v1/ai/tasks", injection).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/ai/tasks",
            json!({"provider":"cursor","prompt":"test"})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for _ in 0..4 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/ai/tasks",
                json!({"provider":"codex","prompt":"test"})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/ai/tasks",
            json!({"provider":"codex","prompt":"test"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

fn fixture() -> (App, tempfile::TempDir) {
    fixture_with_control(true)
}

fn fixture_with_control(control_enabled: bool) -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut config: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
    config.state_dir = dir.path().to_path_buf();
    config.control_enabled = control_enabled;
    let app = App {
        ai: Arc::new(Mutex::new(ai::AiState::default())),
        config: Arc::new(config),
        token: Arc::new(TOKEN.into()),
        demo: true,
        mutation: Arc::new(Mutex::new(())),
        demo_active: Arc::new(Mutex::new(BTreeMap::new())),
        demo_cpu_active: Arc::new(Mutex::new(None)),
        demo_node: Arc::new(Mutex::new("DEMO-HK".into())),
        runtime: Arc::new(Mutex::new(BTreeMap::new())),
        process_watch: Arc::new(Mutex::new(ProcessWatchState::default())),
        cpu_usage: Arc::new(Mutex::new(None)),
        cpu_runtime: Arc::new(Mutex::new(CpuRuntime::default())),
        push: Arc::new(crate::push::PushService::new(
            dir.path().to_path_buf(),
            reqwest::Client::new(),
        )),
        client: reqwest::Client::new(),
    };
    (app, dir)
}

async fn call(app: &App, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = router(app.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let code = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn all_routes_require_authentication() {
    let (app, _dir) = fixture();
    for (method, path) in [
        ("GET", "/api/v1/status"),
        ("GET", "/api/v1/catalog"),
        ("PUT", "/api/v1/selection"),
        ("POST", "/api/v1/miner"),
        ("PUT", "/api/v1/cpu/selection"),
        ("POST", "/api/v1/cpu/miner"),
        ("GET", "/api/v1/cpu/policy"),
        ("PUT", "/api/v1/cpu/policy"),
        ("PUT", "/api/v1/gpus/0/selection"),
        ("POST", "/api/v1/gpus/1/miner"),
        ("GET", "/api/v1/gpus/1/policy"),
        ("PUT", "/api/v1/gpus/0/policy"),
        ("PUT", "/api/v1/push/token"),
        ("GET", "/api/v1/proxy"),
        ("PUT", "/api/v1/proxy"),
        ("GET", "/api/v1/ai"),
        ("POST", "/api/v1/ai/tasks"),
        ("POST", "/api/v1/ai/tasks/test/cancel"),
        ("POST", "/api/v1/ai/worker/poll"),
        ("POST", "/api/v1/ai/worker/result"),
    ] {
        let response = router(app.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn status_reports_external_gpu_processes() {
    let (app, _dir) = fixture();
    let (status, body) = call(&app, "GET", "/api/v1/status", Value::Null).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["gpu_controls"]["0"]["external_pids"], json!([]));
    assert_eq!(body["gpu_controls"]["1"]["external_pids"], json!([]));
    assert_eq!(body["gpu_controls"]["0"]["processes"], json!([]));
    assert_eq!(body["gpu_controls"]["1"]["processes"], json!([]));
}

#[test]
fn process_alerts_only_fire_for_new_outside_processes() {
    let mut watch = ProcessWatchState::default();
    let process = gpu::GpuProcess {
        gpu_id: 1,
        pid: 123,
        process_type: "C".into(),
        name: "python".into(),
        memory_used_mib: Some(64.0),
        executable: Some("/tmp/python".into()),
        within_gpu_root: Some(false),
    };
    let key = (1, 123, "/tmp/python".into());

    assert!(
        update_process_snapshot(&mut watch, BTreeMap::from([(key.clone(), process.clone())]))
            .is_empty()
    );
    assert!(
        update_process_snapshot(&mut watch, BTreeMap::from([(key.clone(), process.clone())]))
            .is_empty()
    );
    assert_eq!(
        update_process_snapshot(&mut watch, BTreeMap::new()).len(),
        0
    );
    assert_eq!(
        update_process_snapshot(&mut watch, BTreeMap::from([(key, process)])).len(),
        1
    );
}

#[tokio::test]
async fn stage_apply_conflict_and_stop() {
    let (app, _dir) = fixture();
    let mut selection =
        json!({"profile_id":"pearl","pool_id":"sg","wallet":"testWallet.worker","revision":0});
    assert_eq!(
        call(&app, "PUT", "/api/v1/selection", selection.clone())
            .await
            .0,
        StatusCode::OK
    );
    let (_, staged) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert!(staged["active"].is_null());
    assert_eq!(staged["desired"]["revision"], 1);
    assert_eq!(
        call(&app, "PUT", "/api/v1/selection", selection.clone())
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/miner",
            json!({"operation":"restart","revision":0})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/miner",
            json!({"operation":"start","revision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    selection["revision"] = json!(1);
    selection["wallet"] = json!("updatedWallet.worker");
    call(&app, "PUT", "/api/v1/selection", selection).await;
    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["active"]["pool_id"], "sg");
    assert_eq!(status["desired"]["pool_id"], "sg");
    assert_eq!(status["active"]["wallet"], "testWallet.worker");
    assert_eq!(status["desired"]["wallet"], "updatedWallet.worker");
    assert_eq!(
        app.config.read_gpu_selection("applied", 0).unwrap().pool_id,
        "sg"
    );
    call(
        &app,
        "POST",
        "/api/v1/miner",
        json!({"operation":"restart","revision":2}),
    )
    .await;
    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["active"]["pool_id"], "sg");
    call(
        &app,
        "POST",
        "/api/v1/miner",
        json!({"operation":"stop","revision":2}),
    )
    .await;
    assert!(app.demo_active.lock().await.is_empty());
}

#[tokio::test]
async fn status_uses_server_auto_start_timer_instead_of_starting_its_own() {
    let (app, _dir) = fixture();
    let since = Instant::now() - Duration::from_secs(AUTO_START_IDLE_SECONDS + 45);
    app.runtime.lock().await.insert(
        0,
        RuntimeState {
            low_util_since: Some(since),
            ..RuntimeState::default()
        },
    );

    let (status_code, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;

    assert_eq!(status_code, StatusCode::OK);
    assert!(status["gpu_controls"]["0"]["idle_seconds"]
        .as_u64()
        .is_some_and(|seconds| seconds >= AUTO_START_IDLE_SECONDS + 45));
    assert_eq!(
        app.runtime.lock().await.get(&0).unwrap().low_util_since,
        Some(since)
    );
}

#[tokio::test]
async fn cpu_mining_is_independent_and_defaults_to_half_threads() {
    let (app, _dir) = fixture();
    let (_, catalog) = call(&app, "GET", "/api/v1/catalog", Value::Null).await;
    assert_eq!(catalog["cpu_profiles"][0]["id"], "xmr-randomx");
    assert_eq!(catalog["cpu_profiles"][0]["algorithm"], "randomx");

    let selected = json!({
        "profile_id":"xmr-randomx",
        "pool_id":"kryptex-us",
        "wallet":"testWallet.worker",
        "revision":0
    });
    let (status, staged) = call(&app, "PUT", "/api/v1/cpu/selection", selected.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(staged["selection"]["threads_percent"], 50);
    assert_eq!(staged["selection"]["revision"], 1);
    assert_eq!(
        call(&app, "PUT", "/api/v1/cpu/selection", selected).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/cpu/selection",
            json!({
                "profile_id":"xmr-randomx",
                "pool_id":"kryptex-us",
                "wallet":"testWallet.worker",
                "threads_percent":101,
                "revision":1
            })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cpu/miner",
            json!({"operation":"start","revision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(app.demo_active.lock().await.is_empty());
    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["cpu_mining"]["service"], "active");
    assert_eq!(status["cpu_mining"]["active"]["threads_percent"], 50);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cpu/miner",
            json!({"operation":"stop","revision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(cpu_service_state(&app).await.unwrap(), "inactive");
}

#[tokio::test]
async fn cpu_policy_defaults_disabled_and_validates_idle_settings() {
    let (app, _dir) = fixture();
    let (code, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(status["cpu_mining"]["policy"]["enabled"], false);
    assert_eq!(status["cpu_mining"]["policy"]["idle_start_enabled"], false);
    assert_eq!(status["cpu_mining"]["policy"]["night_mode_enabled"], false);
    assert_eq!(status["cpu_mining"]["policy"]["idle_threshold_percent"], 25);
    assert_eq!(status["cpu_mining"]["policy"]["idle_duration_minutes"], 10);
    assert_eq!(status["cpu_mining"]["mode"], "normal");

    let (code, saved) = call(
        &app,
        "PUT",
        "/api/v1/cpu/policy",
        json!({
            "enabled":true,
            "idle_start_enabled":true,
            "night_mode_enabled":true,
            "idle_threshold_percent":30,
            "idle_duration_minutes":15
        }),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(saved["policy"]["idle_threshold_percent"], 30);
    assert_eq!(
        app.config.read_cpu_policy().unwrap().idle_duration_minutes,
        15
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/cpu/policy",
            json!({
                "enabled":true,
                "idle_start_enabled":true,
                "night_mode_enabled":false,
                "idle_threshold_percent":0,
                "idle_duration_minutes":15
            })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn cpu_automatic_night_starts_and_stops_at_beijing_schedule() {
    let (app, _dir) = fixture();
    let selected = CpuSelection {
        profile_id: "xmr-randomx".into(),
        pool_id: "kryptex-us".into(),
        wallet: "test.worker".into(),
        threads_percent: 25,
        revision: 1,
    };
    app.config.save_cpu_selection("desired", &selected).unwrap();
    app.config.save_cpu_selection("applied", &selected).unwrap();
    app.config
        .save_cpu_policy(&CpuPolicy {
            enabled: true,
            night_mode_enabled: true,
            ..CpuPolicy::default()
        })
        .unwrap();

    let beijing_0200 = UNIX_EPOCH + Duration::from_secs(18 * 60 * 60);
    cpu_policy_tick(&app, beijing_0200).await.unwrap();
    assert_eq!(app.config.read_cpu_mode().unwrap(), CpuMode::Night);
    assert!(app.demo_cpu_active.lock().await.is_some());

    let beijing_0800 = UNIX_EPOCH + Duration::from_secs(0);
    cpu_policy_tick(&app, beijing_0800).await.unwrap();
    assert_eq!(app.config.read_cpu_mode().unwrap(), CpuMode::Normal);
    assert!(app.demo_cpu_active.lock().await.is_none());
}

#[tokio::test]
async fn manual_cpu_night_stops_at_eight_when_automation_is_off() {
    let (app, _dir) = fixture_with_control(false);
    let selected = CpuSelection {
        profile_id: "xmr-randomx".into(),
        pool_id: "kryptex-us".into(),
        wallet: "test.worker".into(),
        threads_percent: 25,
        revision: 1,
    };
    app.config.save_cpu_selection("desired", &selected).unwrap();
    app.config.save_cpu_selection("applied", &selected).unwrap();
    app.config.save_cpu_mode(CpuMode::ManualNight).unwrap();
    *app.demo_cpu_active.lock().await = Some(selected);

    let beijing_0800 = UNIX_EPOCH + Duration::from_secs(0);
    cpu_policy_tick(&app, beijing_0800).await.unwrap();
    assert_eq!(app.config.read_cpu_mode().unwrap(), CpuMode::Normal);
    assert!(app.demo_cpu_active.lock().await.is_none());
}

#[tokio::test]
async fn catalog_reports_public_miner_capabilities() {
    let (app, _dir) = fixture();
    let (status, catalog) = call(&app, "GET", "/api/v1/catalog", Value::Null).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog["profiles"][0]["id"], "pearl");
    assert_eq!(catalog["profiles"][0]["miner"], "srb-pearl");
    assert_eq!(catalog["profiles"][1]["miner"], "lol-octopus");
    assert!(catalog["profiles"][0].get("binary").is_none());
    for (profile, pool_ids) in [
        (
            &catalog["profiles"][0],
            &["sg", "hk", "prl-eu", "prl-us"][..],
        ),
        (
            &catalog["profiles"][1],
            &[
                "nicehash",
                "octopus-usa",
                "octopus-hk",
                "octopus-br",
                "octopus-in",
                "octopus-jp",
            ][..],
        ),
        (
            &catalog["cpu_profiles"][0],
            &["kryptex-us", "xmr-eu", "xmr-sg", "xmr-hk"][..],
        ),
    ] {
        let pools = profile["pools"].as_array().unwrap();
        for id in pool_ids {
            assert!(pools.iter().any(|pool| pool["id"] == *id), "{id}");
        }
    }
}

#[tokio::test]
async fn gpu_scoped_selections_and_services_are_independent() {
    let (app, _dir) = fixture();
    for (gpu_id, profile_id, pool_id) in [(0, "pearl", "sg"), (1, "octopus", "nicehash")] {
        assert_eq!(
            call(
                &app,
                "PUT",
                &format!("/api/v1/gpus/{gpu_id}/selection"),
                json!({
                    "gpu_id":gpu_id,
                    "profile_id":profile_id,
                    "pool_id":pool_id,
                    "wallet":format!("wallet{gpu_id}.worker"),
                    "power_limit_w":401,
                    "revision":0
                })
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("/api/v1/gpus/{gpu_id}/miner"),
                json!({"operation":"start","revision":1})
            )
            .await
            .0,
            StatusCode::OK
        );
    }

    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["gpu_controls"]["0"]["active"]["profile_id"], "pearl");
    assert!(status["gpu_controls"]["0"]["active"]["power_limit_w"].is_null());
    assert_eq!(
        status["gpu_controls"]["1"]["active"]["profile_id"],
        "octopus"
    );
    assert!(status["gpu_controls"]["1"]["active"]["power_limit_w"].is_null());
    assert_eq!(
        app.config
            .read_gpu_selection("applied", 0)
            .unwrap()
            .profile_id,
        "pearl"
    );
    assert_eq!(
        app.config
            .read_gpu_selection("applied", 1)
            .unwrap()
            .profile_id,
        "octopus"
    );

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/gpus/0/miner",
            json!({"operation":"stop","revision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["gpu_controls"]["0"]["service"], "inactive");
    assert_eq!(status["gpu_controls"]["1"]["service"], "active");
}

#[tokio::test]
async fn policies_are_per_gpu_validated_and_disabled_by_default() {
    let (app, _dir) = fixture();
    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["policies"]["0"]["policy"]["enabled"], false);
    assert_eq!(status["policies"]["0"]["policy"]["intensity"], 10);
    assert_eq!(
        status["policies"]["1"]["policy"]["min_free_memory_mib"],
        2048
    );

    call(
        &app,
        "PUT",
        "/api/v1/gpus/1/selection",
        json!({
            "gpu_id":1,
            "profile_id":"pearl",
            "pool_id":"sg",
            "wallet":"test.worker",
            "revision":0
        }),
    )
    .await;
    let (status, policy) = call(
        &app,
        "PUT",
        "/api/v1/gpus/1/policy",
        json!({
            "gpu_id":1,
            "enabled":true,
            "throttle_enabled":true,
            "night_mode_enabled":false,
            "external_task_monitor":false,
            "min_free_memory_mib":2048,
            "power_limit_w":null,
            "intensity":20
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policy["policy"]["intensity"], 20);
    assert!(policy["policy"]["power_limit_w"].is_null());
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/gpus/1/policy",
            json!({
                "gpu_id":1,
                "enabled":true,
                "throttle_enabled":true,
                "night_mode_enabled":true,
                "external_task_monitor":true,
                "min_free_memory_mib":2048,
                "power_limit_w":250,
                "intensity":18
            })
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, response) = call(
        &app,
        "POST",
        "/api/v1/gpus/1/miner",
        json!({"operation":"start","revision":1,"mode":"throttle"}),
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("does not cap GPU utilization"));
    assert!(!app.demo_active.lock().await.contains_key(&1));
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/gpus/0/policy",
            json!({
                "gpu_id":1,
                "enabled":true,
                "throttle_enabled":true,
                "night_mode_enabled":false,
                "external_task_monitor":false,
                "min_free_memory_mib":2048,
                "power_limit_w":250,
            })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/gpus/1/policy",
            json!({
                "gpu_id":1,
                "enabled":true,
                "throttle_enabled":true,
                "night_mode_enabled":false,
                "external_task_monitor":false,
                "min_free_memory_mib":500,
                "power_limit_w":250,
            })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn gpu_starts_use_mode_intensity_and_ignore_legacy_power_limits() {
    let (app, _dir) = fixture();
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/selection",
            json!({
                "profile_id":"pearl",
                "pool_id":"sg",
                "wallet":"testWallet.worker",
                "revision":0
            })
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/miner",
            json!({"operation":"restart","revision":1,"mode":"normal"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        app.config
            .read_gpu_selection("applied", 0)
            .unwrap()
            .gpu_intensity,
        None
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/miner",
            json!({"operation":"restart","revision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        app.config
            .read_gpu_selection("applied", 0)
            .unwrap()
            .gpu_intensity,
        None
    );
}

#[tokio::test]
async fn manual_throttle_start_is_disabled_without_changing_gpu_mode() {
    let (app, _dir) = fixture();
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/gpus/0/selection",
            json!({
                "gpu_id":0,
                "profile_id":"pearl",
                "pool_id":"sg",
                "wallet":"testWallet.worker",
                "revision":0
            })
        )
        .await
        .0,
        StatusCode::OK
    );

    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/gpus/0/miner",
        json!({"operation":"start","revision":1,"mode":"throttle"}),
    )
    .await;

    assert_eq!(status, StatusCode::GONE);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("does not cap GPU utilization"));
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::Normal);
    assert!(!app.demo_active.lock().await.contains_key(&0));
}

#[test]
fn manual_full_start_can_share_but_automatic_start_remains_exclusive() {
    assert!(!requires_exclusive_gpu(GpuMode::Normal, true));
    assert!(requires_exclusive_gpu(GpuMode::Throttle, true));
    assert!(requires_exclusive_gpu(GpuMode::Throttle, false));
    assert!(requires_exclusive_gpu(GpuMode::Night, false));
    assert!(requires_exclusive_gpu(GpuMode::Night, true));
    assert!(!requires_exclusive_gpu(GpuMode::ManualNight, true));
    assert!(!is_automatic_mode(GpuMode::ManualNight));
    assert!(!should_stop_for_external_task(GpuMode::ManualNight, true));
    assert_eq!(srbminer_intensity(GpuMode::ManualNight, 1).unwrap(), 31);
}

#[test]
fn manual_throttle_is_persisted_separately_and_survives_automation_disable() {
    assert_eq!(
        mode_for_action(
            Operation::Start,
            "inactive",
            GpuMode::Throttle,
            GpuMode::Normal,
            true,
            false,
        ),
        GpuMode::ManualThrottle
    );
    assert_eq!(
        mode_for_action(
            Operation::Restart,
            "active",
            GpuMode::Normal,
            GpuMode::Throttle,
            false,
            false,
        ),
        GpuMode::Normal
    );
    assert_eq!(
        mode_for_action(
            Operation::Stop,
            "active",
            GpuMode::Normal,
            GpuMode::ManualNight,
            false,
            false,
        ),
        GpuMode::ManualNight
    );
    assert_eq!(
        mode_for_action(
            Operation::Stop,
            "active",
            GpuMode::Normal,
            GpuMode::ManualNight,
            false,
            true,
        ),
        GpuMode::Normal
    );
    assert!(!is_automatic_mode(GpuMode::ManualThrottle));
    assert!(is_automatic_mode(GpuMode::Throttle));
    assert!(!should_stop_for_external_task(
        GpuMode::ManualThrottle,
        true
    ));
    assert!(should_stop_for_external_task(GpuMode::Throttle, false));
    assert!(should_stop_for_external_task(GpuMode::Normal, true));
    assert!(should_auto_start(true, AUTO_START_IDLE_SECONDS, false));
    assert!(!should_auto_start(true, AUTO_START_IDLE_SECONDS, true));
    assert!(!should_auto_start(false, AUTO_START_IDLE_SECONDS, false));
    assert!(!should_auto_start(true, AUTO_START_IDLE_SECONDS - 1, false));
    assert!(should_track_auto_start_idle(true, Some(0.0), false));
    assert!(!should_track_auto_start_idle(true, Some(4.99), true));
    assert!(!should_track_auto_start_idle(false, Some(0.0), false));
    assert!(should_track_auto_start_idle(true, Some(24.99), false));
    assert!(!should_track_auto_start_idle(true, Some(25.0), false));
    assert!(!should_track_auto_start_idle(true, None, false));
    assert_eq!(srbminer_intensity(GpuMode::ManualThrottle, 10).unwrap(), 10);
}

#[test]
fn throttle_mode_is_preserved() {
    let (app, _dir) = fixture();
    assert_eq!(app.config.read_gpu_policy(0).unwrap().intensity, 10);
    std::fs::write(app.config.state_dir.join("gpu0-mode.json"), "\"throttle\"").unwrap();
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::Throttle);
    assert_eq!(
        std::fs::read_to_string(app.config.state_dir.join("gpu0-mode.json")).unwrap(),
        "\"throttle\""
    );
}

#[test]
fn full_and_throttle_modes_select_expected_srbminer_intensity() {
    assert_eq!(srbminer_intensity(GpuMode::Normal, 10).unwrap(), 31);
    assert_eq!(srbminer_intensity(GpuMode::Throttle, 10).unwrap(), 10);
    assert_eq!(srbminer_intensity(GpuMode::Throttle, 27).unwrap(), 27);
    assert!(srbminer_intensity(GpuMode::Throttle, 32).is_err());
}

#[test]
fn beijing_night_schedule_has_inclusive_start_and_exclusive_end() {
    let beijing = |hour: u64, minute: u64| {
        let local = hour * 3600 + minute * 60;
        let utc = (local + 86_400 - 8 * 3600) % 86_400;
        UNIX_EPOCH + Duration::from_secs(utc)
    };
    assert!(!is_china_night(beijing(0, 59)).unwrap());
    assert!(is_china_night(beijing(1, 0)).unwrap());
    assert!(is_china_night(beijing(7, 59)).unwrap());
    assert!(!is_china_night(beijing(8, 0)).unwrap());
}

#[tokio::test]
async fn manual_night_stops_at_eight_even_when_automation_is_disabled() {
    let (app, _dir) = fixture_with_control(false);
    let selection = Selection {
        gpu_id: 0,
        profile_id: "pearl".into(),
        pool_id: "sg".into(),
        wallet: "test.worker".into(),
        power_limit_w: None,
        gpu_intensity: None,
        revision: 1,
    };
    app.config
        .save_gpu_selection("desired", &selection)
        .unwrap();
    app.config
        .save_gpu_selection("applied", &selection)
        .unwrap();
    app.config.save_gpu_mode(0, GpuMode::ManualNight).unwrap();
    app.demo_active.lock().await.insert(0, selection);

    let beijing_0800 = UNIX_EPOCH + Duration::from_secs(8 * 60 * 60 + 86_400 - 8 * 60 * 60);
    policy_tick_with_process_check_at(&app, &gpu::demo(), beijing_0800, |_| async {
        panic!("expired manual night mode must not check external GPU processes");
        #[allow(unreachable_code)]
        Ok(Vec::new())
    })
    .await
    .unwrap();

    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::Normal);
    assert!(!app.demo_active.lock().await.contains_key(&0));
}

#[tokio::test]
async fn manual_night_ignores_automation_and_external_tasks_during_night_window() {
    let (app, _dir) = fixture();
    let (code, _) = call(
        &app,
        "PUT",
        "/api/v1/gpus/0/selection",
        json!({"gpu_id":0,"profile_id":"pearl","pool_id":"sg","wallet":"test.worker","revision":0}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let mut policy = GpuPolicy::new(0);
    app.config.save_gpu_policy(&policy).unwrap();
    let start = json!({"operation":"restart","revision":1,"mode":"manual_night"});
    let (code, response) = call(&app, "POST", "/api/v1/gpus/0/miner", start).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(response["mode"], "manual_night");
    let mut gpus = gpu::demo();
    gpus[0].utilization = Some(100.0);
    let beijing_0200 = UNIX_EPOCH + Duration::from_secs(2 * 60 * 60 + 86_400 - 8 * 60 * 60);
    for enabled in [false, true] {
        policy.enabled = enabled;
        policy.night_mode_enabled = enabled;
        policy.external_task_monitor = true;
        app.config.save_gpu_policy(&policy).unwrap();
        policy_tick_with_process_check_at(&app, &gpus, beijing_0200, |_| async {
            panic!("manual night must not inspect or yield to external processes");
            #[allow(unreachable_code)]
            Ok(Vec::new())
        })
        .await
        .unwrap();
        assert!(app.demo_active.lock().await.contains_key(&0));
        assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::ManualNight);
    }
    // An automatic action queued before manual takeover must not stop it later.
    auto_action(&app, 0, Operation::Stop, GpuMode::Normal)
        .await
        .unwrap();
    assert!(app.demo_active.lock().await.contains_key(&0));
    auto_action(&app, 0, Operation::Restart, GpuMode::Night)
        .await
        .unwrap();
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::ManualNight);
    policy.enabled = false;
    app.config.save_gpu_policy(&policy).unwrap();
    gpus[0].memory_used_mib = gpus[0].memory_total_mib;
    policy_tick_with_process_check_at(&app, &gpus, beijing_0200, |_| async { Ok(Vec::new()) })
        .await
        .unwrap();
    assert!(!app.demo_active.lock().await.contains_key(&0));
    assert!(app.runtime.lock().await[&0].memory_blocked);
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::ManualNight);
    // Free memory recovery must not restart a protected manual session.
    policy_tick_with_process_check_at(&app, &gpu::demo(), beijing_0200, |_| async {
        Ok(Vec::new())
    })
    .await
    .unwrap();
    assert!(!app.demo_active.lock().await.contains_key(&0));
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::ManualNight);
    let (code, _) = call(
        &app,
        "POST",
        "/api/v1/gpus/0/miner",
        json!({"operation":"stop","revision":1}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::Normal);
    assert!(!app.config.read_gpu_policy(0).unwrap().enabled);
}

#[tokio::test]
async fn manual_night_rejects_insufficient_memory_without_changing_mode_and_stops_on_missing_metrics(
) {
    let (app, _dir) = fixture();
    call(
        &app,
        "PUT",
        "/api/v1/gpus/0/selection",
        json!({"gpu_id":0,"profile_id":"pearl","pool_id":"sg","wallet":"test.worker","revision":0}),
    )
    .await;
    let mut policy = GpuPolicy::new(0);
    policy.min_free_memory_mib = 16384;
    app.config.save_gpu_policy(&policy).unwrap();
    let start = json!({"operation":"start","revision":1,"mode":"manual_night"});
    assert_eq!(
        call(&app, "POST", "/api/v1/gpus/0/miner", start.clone())
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::Normal);
    assert!(!app.demo_active.lock().await.contains_key(&0));
    policy.min_free_memory_mib = 2048;
    app.config.save_gpu_policy(&policy).unwrap();
    assert_eq!(
        call(&app, "POST", "/api/v1/gpus/0/miner", start).await.0,
        StatusCode::OK
    );
    let beijing_0200 = UNIX_EPOCH + Duration::from_secs(2 * 60 * 60 + 86_400 - 8 * 60 * 60);
    policy_tick_with_process_check_at(&app, &[], beijing_0200, |_| async { Ok(Vec::new()) })
        .await
        .unwrap();
    assert!(!app.demo_active.lock().await.contains_key(&0));
    assert_eq!(app.config.read_gpu_mode(0).unwrap(), GpuMode::ManualNight);
    assert!(app.runtime.lock().await[&0].memory_blocked);
}

#[tokio::test]
async fn night_miner_yields_to_user_compute_processes() {
    let (app, _dir) = fixture();
    let mut policy = GpuPolicy::new(0);
    policy.enabled = true;
    policy.night_mode_enabled = true;
    app.config.save_gpu_policy(&policy).unwrap();
    let (code, _) = call(
        &app,
        "PUT",
        "/api/v1/gpus/0/selection",
        json!({"gpu_id":0,"profile_id":"pearl","pool_id":"sg","wallet":"test.worker","revision":0}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    auto_action(&app, 0, Operation::Start, GpuMode::Night)
        .await
        .unwrap();
    assert!(app.demo_active.lock().await.contains_key(&0));
    let mut gpus = gpu::demo();
    gpus[0].utilization = Some(0.0);
    policy_tick_with_process_check(&app, &gpus, |_| async { Ok(vec![12345]) })
        .await
        .unwrap();
    assert!(!app.demo_active.lock().await.contains_key(&0));
    assert!(app.runtime.lock().await[&0].external_task);
    assert!(app.runtime.lock().await[&0].low_util_since.is_none());
    policy_tick_with_process_check(&app, &gpus, |_| async { Ok(vec![12345]) })
        .await
        .unwrap();
    assert!(!app.demo_active.lock().await.contains_key(&0));
}

#[tokio::test]
async fn idle_policy_preserves_timer_and_starts_after_ten_minutes() {
    let (app, _dir) = fixture();
    let mut policy = GpuPolicy::new(0);
    policy.enabled = true;
    policy.throttle_enabled = true;
    app.config.save_gpu_policy(&policy).unwrap();
    let (code, _) = call(
        &app,
        "PUT",
        "/api/v1/gpus/0/selection",
        json!({"gpu_id":0,"profile_id":"pearl","pool_id":"sg","wallet":"test.worker","revision":0}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let mut gpus = gpu::demo();
    gpus[0].utilization = Some(0.0);
    policy_tick_with_gpus(&app, &gpus).await.unwrap();
    assert!(app.runtime.lock().await[&0].low_util_since.is_some());
    let since = Instant::now() - Duration::from_secs(AUTO_START_IDLE_SECONDS - 5);
    app.runtime.lock().await.get_mut(&0).unwrap().low_util_since = Some(since);
    gpus[0].utilization = Some(24.99);
    policy_tick_with_gpus(&app, &gpus).await.unwrap();
    assert_eq!(app.runtime.lock().await[&0].low_util_since, Some(since));
    assert!(!app.demo_active.lock().await.contains_key(&0));
    app.runtime.lock().await.get_mut(&0).unwrap().low_util_since =
        Some(Instant::now() - Duration::from_secs(AUTO_START_IDLE_SECONDS));
    policy_tick_with_gpus(&app, &gpus).await.unwrap();
    assert!(app.demo_active.lock().await.contains_key(&0));
    assert!(!app.demo_active.lock().await.contains_key(&1));
    policy_tick_with_gpus(&app, &gpus).await.unwrap();
    assert!(app.runtime.lock().await[&0].low_util_since.is_none());
}

#[tokio::test]
async fn idle_policy_resets_timer_when_low_utilization_is_interrupted() {
    let (app, _dir) = fixture();
    let mut policy = GpuPolicy::new(0);
    policy.enabled = true;
    policy.throttle_enabled = true;
    app.config.save_gpu_policy(&policy).unwrap();
    for utilization in [Some(25.0), Some(90.0), None] {
        let mut gpus = gpu::demo();
        gpus[0].utilization = utilization;
        app.runtime
            .lock()
            .await
            .entry(0)
            .or_default()
            .low_util_since = Some(Instant::now() - Duration::from_secs(AUTO_START_IDLE_SECONDS));
        policy_tick_with_gpus(&app, &gpus).await.unwrap();
        assert!(app.runtime.lock().await[&0].low_util_since.is_none());
    }
    app.runtime
        .lock()
        .await
        .entry(0)
        .or_default()
        .low_util_since = Some(Instant::now());
    policy_tick_with_gpus(&app, &[]).await.unwrap();
    assert!(app.runtime.lock().await[&0].low_util_since.is_none());
    policy.throttle_enabled = false;
    app.config.save_gpu_policy(&policy).unwrap();
    let mut gpus = gpu::demo();
    gpus[0].utilization = Some(0.0);
    policy_tick_with_gpus(&app, &gpus).await.unwrap();
    assert!(app.runtime.lock().await[&0].low_util_since.is_none());
}

#[tokio::test]
async fn demo_policy_tick_does_not_query_host_gpu_processes() {
    let (app, _dir) = fixture();
    app.config
        .save_gpu_policy(&GpuPolicy {
            gpu_id: 1,
            enabled: true,
            throttle_enabled: false,
            night_mode_enabled: false,
            external_task_monitor: true,
            min_free_memory_mib: 2048,
            power_limit_w: None,
            intensity: 20,
        })
        .unwrap();
    policy_tick(&app).await.unwrap();
    assert_eq!(app.config.read_gpu_mode(1).unwrap(), GpuMode::Normal);
    assert!(!app.demo_active.lock().await.contains_key(&1));
}

#[tokio::test]
async fn push_token_routes_register_deduplicate_and_remove_tokens() {
    let (app, dir) = fixture();
    let token = "test-firebase-device-token-with-more-than-32-characters";
    for _ in 0..2 {
        assert_eq!(
            call(&app, "PUT", "/api/v1/push/token", json!({"token":token}))
                .await
                .0,
            StatusCode::OK
        );
    }
    let saved: Vec<String> =
        serde_json::from_slice(&std::fs::read(dir.path().join("push-tokens.json")).unwrap())
            .unwrap();
    assert_eq!(saved, [token]);
    assert_eq!(
        call(&app, "DELETE", "/api/v1/push/token", json!({"token":token}))
            .await
            .0,
        StatusCode::OK
    );
    let saved: Vec<String> =
        serde_json::from_slice(&std::fs::read(dir.path().join("push-tokens.json")).unwrap())
            .unwrap();
    assert!(saved.is_empty());
}

#[tokio::test]
async fn rejects_wallet_injection_wrong_pool_and_disabled_control() {
    let (mut app, _dir) = fixture();
    for wallet in [
        "-flag",
        "wallet\n--gpu-id 1",
        "$(touch /tmp/pwn)",
        "a,b",
        "",
    ] {
        assert_eq!(
            call(
                &app,
                "PUT",
                "/api/v1/selection",
                json!({"profile_id":"pearl","pool_id":"sg","wallet":wallet,"revision":0})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(
                &app,
                "PUT",
                "/api/v1/gpus/2/selection",
                json!({
                    "gpu_id":2,
                    "profile_id":"pearl",
                    "pool_id":"sg",
                    "wallet":"valid.worker",
                    "revision":0
                })
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/selection",
            json!({"profile_id":"pearl","pool_id":"nicehash","wallet":"valid","revision":0})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    Arc::make_mut(&mut app.config).control_enabled = false;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/miner",
            json!({"operation":"stop","revision":0})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "PUT", "/api/v1/proxy", json!({"name":"DEMO-SG"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn proxy_membership_and_current_selection() {
    let (app, _dir) = fixture();
    for name in ["DIRECT", "nonexistent", "../other-group"] {
        assert_eq!(
            call(&app, "PUT", "/api/v1/proxy", json!({"name":name}))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    call(&app, "PUT", "/api/v1/proxy", json!({"name":"DEMO-SG"})).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/proxy", Value::Null).await.1["current"],
        "DEMO-SG"
    );
}

#[test]
fn unavailable_gpu_values_remain_null_and_bad_rows_fail() {
    let gpus =
        gpu::parse("0, GPU-test, RTX 3080 Ti, 99, 512, 12288, 72, 300.5, 350, 100, 400, [N/A], P2")
            .unwrap();
    assert_eq!(gpus[0].fan_percent, None);
    assert_eq!(gpus[0].power_max_limit_w, Some(400.0));
    assert_eq!(gpus[0].utilization, Some(99.0));
    assert!(gpu::parse("0, broken").is_err());
}

#[test]
fn configuration_rejects_path_escape_and_remote_controller() {
    let (app, _dir) = fixture();
    let mut config = (*app.config).clone();
    config.profiles[0].binary = "../../bin/sh".into();
    assert!(config.validate().is_err());
    config = (*app.config).clone();
    config.proxy.as_mut().unwrap().endpoint = "https://attacker.example".into();
    assert!(config.validate().is_err());
}
