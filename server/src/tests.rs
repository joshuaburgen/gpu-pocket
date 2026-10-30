use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use std::collections::BTreeMap;
use tower::ServiceExt;

const TOKEN: &str = "test-only-32-characters-abcdefghijk";

fn fixture() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut config: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
    config.state_dir = dir.path().to_path_buf();
    config.control_enabled = true;
    let app = App {
        config: Arc::new(config),
        token: Arc::new(TOKEN.into()),
        demo: true,
        mutation: Arc::new(Mutex::new(())),
        demo_active: Arc::new(Mutex::new(BTreeMap::new())),
        demo_node: Arc::new(Mutex::new("DEMO-HK".into())),
        runtime: Arc::new(Mutex::new(BTreeMap::new())),
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
        ("PUT", "/api/v1/gpus/0/selection"),
        ("POST", "/api/v1/gpus/1/miner"),
        ("GET", "/api/v1/gpus/1/policy"),
        ("PUT", "/api/v1/gpus/0/policy"),
        ("PUT", "/api/v1/push/token"),
        ("GET", "/api/v1/proxy"),
        ("PUT", "/api/v1/proxy"),
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
    selection["pool_id"] = json!("hk");
    call(&app, "PUT", "/api/v1/selection", selection).await;
    let (_, status) = call(&app, "GET", "/api/v1/status", Value::Null).await;
    assert_eq!(status["active"]["pool_id"], "sg");
    assert_eq!(status["desired"]["pool_id"], "hk");
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
    assert_eq!(status["active"]["pool_id"], "hk");
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
async fn gpu_scoped_selections_and_services_are_independent() {
    let (app, _dir) = fixture();
    for (gpu_id, profile_id, pool_id, power_limit_w) in
        [(0, "pearl", "sg", 250), (1, "octopus", "nicehash", 300)]
    {
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
                    "power_limit_w":power_limit_w,
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
    assert_eq!(status["gpu_controls"]["0"]["active"]["power_limit_w"], 250);
    assert_eq!(
        status["gpu_controls"]["1"]["active"]["profile_id"],
        "octopus"
    );
    assert_eq!(status["gpu_controls"]["1"]["active"]["power_limit_w"], 300);
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
                "intensity":20
            })
        )
        .await
        .0,
        StatusCode::OK
    );
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
                "intensity":20
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
                "min_free_memory_mib":2048,
                "power_limit_w":250,
                "intensity":99
            })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
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
                "/api/v1/gpus/1/selection",
                json!({
                    "gpu_id":1,
                    "profile_id":"pearl",
                    "pool_id":"sg",
                    "wallet":"valid.worker",
                    "power_limit_w":401,
                    "revision":0
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
