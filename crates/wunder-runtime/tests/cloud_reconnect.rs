//! Cloud channel reconnect tests: silent single-flight token recovery on 401,
//! refresh-token reuse forcing a local logout, network-failure handling
//! (reconnecting without a false "expired"), the idle heartbeat empty batch
//! and the proactive renewal threshold.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wunder_server::cloud::{shared as cloud_shared, CloudSessionExpired};
use wunder_server::config::LlmModelConfig;
use wunder_server::config_store::ConfigStore;
use wunder_server::llm::{build_llm_client, ChatMessage};

const TOKEN_V1: &str = "wund_tok_v1";
const TOKEN_V2: &str = "wund_tok_v2";
const REFRESH_V1: &str = "wund_ref_v1";
const REFRESH_V2: &str = "wund_ref_v2";
const TEST_DEVICE: &str = "dev-reconnect-1";

#[derive(Debug, Clone)]
struct MockRequest {
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

#[derive(Debug, Clone)]
struct MockResponse {
    /// 0 means: accept the connection then drop it without answering,
    /// simulating a network-level failure.
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl MockResponse {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.to_string(),
        }
    }

    fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    fn connection_dropped() -> Self {
        Self {
            status: 0,
            headers: Vec::new(),
            body: String::new(),
        }
    }
}

type MockHandler = Arc<dyn Fn(MockRequest) -> MockResponse + Send + Sync>;

async fn spawn_mock_server(handler: MockHandler) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(value) => value,
                Err(_) => return,
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let _ = serve_connection(stream, handler).await;
            });
        }
    });
    format!("http://{addr}")
}

async fn serve_connection(
    mut stream: tokio::net::TcpStream,
    handler: MockHandler,
) -> std::io::Result<()> {
    let mut buffer: Vec<u8> = Vec::new();
    let header_end;
    loop {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            header_end = position + 4;
            break;
        }
        if buffer.len() > 128 * 1024 {
            return Ok(());
        }
    }
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8_lossy(&buffer[header_end..])
        .chars()
        .take(content_length)
        .collect();

    let response = handler(MockRequest {
        path,
        headers,
        body,
    });
    if response.status == 0 {
        // Simulated network failure: drop without responding.
        return Ok(());
    }
    let reason = match response.status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "OK",
    };
    let mut out = format!(
        "HTTP/1.1 {} {reason}\r\nconnection: close\r\n",
        response.status
    );
    for (name, value) in &response.headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!("content-length: {}\r\n\r\n", response.body.len()));
    stream.write_all(out.as_bytes()).await?;
    stream.write_all(response.body.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn bearer(request: &MockRequest) -> Option<String> {
    request.headers.get("authorization").cloned()
}

fn refresh_body(access: &str, refresh: &str, ttl_secs: f64) -> Value {
    json!({
        "data": {
            "access_token": access,
            "refresh_token": refresh,
            "expires_at": now_unix() + ttl_secs,
            "user": {"user_id": "u-reconnect", "username": "tester"}
        }
    })
}

fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

/// Full mock for the login flow; the login response carries the rotating
/// refresh token pair so the engine can renew silently. `account_status`
/// flips the account endpoint to 401 to trigger the recovery path.
fn cloud_login_handler(account_status: Arc<AtomicUsize>) -> MockHandler {
    Arc::new(move |request: MockRequest| match request.path.as_str() {
        "/wunder/auth/login" => MockResponse::json(
            200,
            json!({
                "data": {
                    "access_token": TOKEN_V1,
                    "refresh_token": REFRESH_V1,
                    "expires_at": now_unix() + 90.0 * 24.0 * 3600.0,
                    "user": {"user_id": "u-reconnect", "username": "tester"}
                }
            }),
        ),
        "/wunder/auth/me/preferences" => MockResponse::json(200, json!({"data": {}})),
        "/wunder/cloud/devices" => MockResponse::json(
            200,
            json!({"data": {"device_id": TEST_DEVICE, "max_concurrent_calls": 2}}),
        ),
        "/wunder/cloud/account" => {
            let call = account_status.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                MockResponse::json(
                    200,
                    json!({
                        "data": {
                            "user_id": "u-reconnect",
                            "username": "tester",
                            "quota": {"balance": 10, "granted_total": 20, "used_total": 10, "daily_grant": 5},
                            "concurrency": {"max_per_user": 2, "active": 0, "queued": 0}
                        }
                    }),
                )
            } else {
                MockResponse::json(
                    401,
                    json!({"error": {"code": "SESSION_EXPIRED", "message": "session expired"}}),
                )
                .with_header("x-error-code", "SESSION_EXPIRED")
            }
        }
        "/wunder/cloud/v1/models" => MockResponse::json(
            200,
            json!({"object": "list", "data": [{"id": "m-alpha", "object": "model"}]}),
        ),
        "/wunder/cloud/logs" => {
            MockResponse::json(200, json!({"data": {"accepted": 0, "last_seq": 0}}))
        }
        _ => MockResponse::json(
            404,
            json!({"error": {"code": "NOT_FOUND", "message": "missing route"}}),
        ),
    })
}

fn cloud_llm_config(base_url: &str) -> LlmModelConfig {
    LlmModelConfig {
        provider: Some("wunder_cloud".to_string()),
        base_url: Some(base_url.to_string()),
        api_key: Some(TOKEN_V1.to_string()),
        model: Some("m-alpha".to_string()),
        model_type: Some("llm".to_string()),
        ..Default::default()
    }
}

fn test_message() -> ChatMessage {
    ChatMessage {
        role: "user".to_string(),
        content: Value::String("ping".to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

fn write_session_file(home: &std::path::Path, token: &str, refresh: &str, expires_in: f64) {
    let session = json!({
        "server": "http://127.0.0.1:9999",
        "user_id": "u-reconnect",
        "username": "tester",
        "scope": "local_desktop",
        "token": token,
        "refresh_token": refresh,
        "token_expires_at": now_unix() + expires_in,
        "device_id": TEST_DEVICE,
        "device_name": "pc",
        "client": "desktop",
        "logged_in_at": now_unix(),
        "log_report": {"enabled": true, "level": "warn", "last_synced_seq": 0},
        "preferences_sync_enabled": true,
    });
    let dir = home.join("config");
    std::fs::create_dir_all(&dir).expect("create config dir");
    std::fs::write(dir.join("cloud.session.json"), session.to_string()).expect("write session");
}

/// One shared cloud service per process; serialize every test that touches it.
static CLOUD_TEST_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn silent_refresh_recovers_after_401_and_retries_with_new_token() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));

    let refresh_hits = Arc::new(AtomicUsize::new(0));
    let chat_calls = Arc::new(AtomicUsize::new(0));
    let seen_authorizations: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let account_status = Arc::new(AtomicUsize::new(0));
    let (refresh_hits_clone, chat_calls_clone, seen_clone) = (
        refresh_hits.clone(),
        chat_calls.clone(),
        seen_authorizations.clone(),
    );
    // One combined mock: the login flow plus the 401→refresh→retry chat path.
    let handler: MockHandler = Arc::new(move |request: MockRequest| {
        let presented_refresh = serde_json::from_str::<Value>(&request.body)
            .ok()
            .and_then(|body| body["refresh_token"].as_str().map(str::to_string));
        let bearer_token = bearer(&request);
        match request.path.as_str() {
            "/wunder/auth/login" => MockResponse::json(
                200,
                json!({
                    "data": {
                        "access_token": TOKEN_V1,
                        "refresh_token": REFRESH_V1,
                        "expires_at": now_unix() + 90.0 * 24.0 * 3600.0,
                        "user": {"user_id": "u-reconnect", "username": "tester"}
                    }
                }),
            ),
            "/wunder/auth/me/preferences" => MockResponse::json(200, json!({"data": {}})),
            "/wunder/cloud/devices" => MockResponse::json(
                200,
                json!({"data": {"device_id": TEST_DEVICE, "max_concurrent_calls": 2}}),
            ),
            "/wunder/cloud/account" => {
                let call = account_status.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    MockResponse::json(
                        200,
                        json!({
                            "data": {
                                "user_id": "u-reconnect",
                                "username": "tester",
                                "quota": {"balance": 10, "granted_total": 20, "used_total": 10, "daily_grant": 5},
                                "concurrency": {"max_per_user": 2, "active": 0, "queued": 0}
                            }
                        }),
                    )
                } else {
                    MockResponse::json(
                        401,
                        json!({"error": {"code": "SESSION_EXPIRED", "message": "session expired"}}),
                    )
                    .with_header("x-error-code", "SESSION_EXPIRED")
                }
            }
            "/wunder/cloud/v1/models" => MockResponse::json(
                200,
                json!({"object": "list", "data": [{"id": "m-alpha", "object": "model"}]}),
            ),
            "/wunder/cloud/logs" => {
                MockResponse::json(200, json!({"data": {"accepted": 0, "last_seq": 0}}))
            }
            "/wunder/auth/refresh" => {
                refresh_hits_clone.fetch_add(1, Ordering::SeqCst);
                assert_eq!(presented_refresh.as_deref(), Some(REFRESH_V1));
                MockResponse::json(
                    200,
                    refresh_body(TOKEN_V2, REFRESH_V2, 90.0 * 24.0 * 3600.0),
                )
            }
            path if path.ends_with("/chat/completions") => {
                seen_clone
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .push(bearer_token.unwrap_or_default());
                let call = chat_calls_clone.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    MockResponse::json(
                        401,
                        json!({"error": {"code": "SESSION_EXPIRED", "message": "session expired"}}),
                    )
                    .with_header("x-error-code", "SESSION_EXPIRED")
                } else {
                    MockResponse::json(
                        200,
                        json!({
                            "id": "chatcmpl-1",
                            "choices": [{"message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
                            "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
                        }),
                    )
                }
            }
            _ => MockResponse::json(
                404,
                json!({"error": {"code": "NOT_FOUND", "message": "missing route"}}),
            ),
        }
    });
    let server = spawn_mock_server(handler).await;

    let status = cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");
    assert!(status.logged_in);
    assert_eq!(status.connection, "online");

    let client = build_llm_client(
        &cloud_llm_config(&format!("{server}/wunder/cloud/v1")),
        reqwest::Client::new(),
    );

    let response = client
        .complete(&[test_message()])
        .await
        .expect("recovered call completes");
    assert_eq!(response.content, "pong");
    assert_eq!(
        refresh_hits.load(Ordering::SeqCst),
        1,
        "exactly one refresh"
    );
    let authorizations = seen_authorizations
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    assert_eq!(authorizations.len(), 2);
    assert_eq!(authorizations[0], format!("Bearer {TOKEN_V1}"));
    assert_eq!(
        authorizations[1],
        format!("Bearer {TOKEN_V2}"),
        "retry must present the rotated token"
    );
    assert_eq!(cloud.status().connection, "online");
    assert_eq!(cloud.status().expired, false);

    // The rotated pair replaced the old one on disk.
    let raw = std::fs::read_to_string(home.path().join("config/cloud.session.json"))
        .expect("session file");
    let persisted: Value = serde_json::from_str(&raw).expect("session json");
    assert_eq!(persisted["token"], TOKEN_V2);
    assert_eq!(persisted["refresh_token"], REFRESH_V2);

    cloud.logout(&store).await.expect("logout");
}

#[tokio::test]
async fn refresh_reuse_forces_local_logout() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));
    // First account call answers the login flow; later ones answer 401 to
    // trigger recovery.
    let account_status = Arc::new(AtomicUsize::new(0));
    let server = spawn_mock_server(cloud_login_handler(account_status.clone())).await;
    cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");

    let session_path = home.path().join("config/cloud.session.json");
    assert!(session_path.exists());

    let refresh_hits = Arc::new(AtomicUsize::new(0));
    let (refresh_hits_clone, account_status_clone) = (refresh_hits.clone(), account_status.clone());
    let reuse_handler: MockHandler = Arc::new(move |request: MockRequest| {
        if request.path == "/wunder/auth/refresh" {
            refresh_hits_clone.fetch_add(1, Ordering::SeqCst);
            return MockResponse::json(
                401,
                json!({"error": {"code": "REFRESH_TOKEN_REUSED", "message": "reuse"}}),
            )
            .with_header("x-error-code", "REFRESH_TOKEN_REUSED");
        }
        if request.path == "/wunder/cloud/account" {
            if account_status_clone.fetch_add(1, Ordering::SeqCst) == 0 {
                return MockResponse::json(200, json!({"data": {"quota": {}, "concurrency": {}}}));
            }
            return MockResponse::json(
                401,
                json!({"error": {"code": "SESSION_EXPIRED", "message": "session expired"}}),
            )
            .with_header("x-error-code", "SESSION_EXPIRED");
        }
        MockResponse::json(200, json!({"data": {}}))
    });
    let server = spawn_mock_server(reuse_handler).await;
    // Point the session at the reuse mock.
    let mut session = cloud.session().expect("session");
    session.server = server.clone();
    cloud.install_session(session);

    let err = cloud
        .refresh_account()
        .await
        .expect_err("recovery is terminal after reuse detection");
    assert!(err.downcast_ref::<CloudSessionExpired>().is_some());

    assert_eq!(refresh_hits.load(Ordering::SeqCst), 1);
    assert!(!session_path.exists(), "session file must be wiped");
    let status = cloud.status();
    assert!(!status.logged_in);
    assert_eq!(status.connection, "logged_out");

    // No further request may carry the revoked token.
    let seen = refresh_hits.load(Ordering::SeqCst);
    let _ = cloud.refresh_account().await;
    assert_eq!(
        refresh_hits.load(Ordering::SeqCst),
        seen,
        "no refresh attempt without a session"
    );
    cloud.logout(&store).await.expect("logout cleanup");
}

#[tokio::test]
async fn refresh_invalid_marks_expired_but_keeps_session() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));
    let account_status = Arc::new(AtomicUsize::new(0));
    let server = spawn_mock_server(cloud_login_handler(account_status.clone())).await;
    cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");

    let invalid_handler: MockHandler = Arc::new(|_request: MockRequest| {
        MockResponse::json(
            401,
            json!({"error": {"code": "REFRESH_TOKEN_INVALID", "message": "invalid"}}),
        )
        .with_header("x-error-code", "REFRESH_TOKEN_INVALID")
    });
    let server = spawn_mock_server(invalid_handler).await;
    let mut session = cloud.session().expect("session");
    session.server = server.clone();
    cloud.install_session(session);

    let recovered = cloud.recover_auth(TOKEN_V1).await.expect("recover result");
    assert!(!recovered, "invalid refresh token is unrecoverable");
    let status = cloud.status();
    assert!(status.expired);
    assert_eq!(status.connection, "expired");
    assert!(
        home.path().join("config/cloud.session.json").exists(),
        "session file stays so the user can re-login"
    );
    cloud.logout(&store).await.expect("logout");
}

#[tokio::test]
async fn concurrent_recover_auth_is_single_flight() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));
    let account_status = Arc::new(AtomicUsize::new(0));
    let server = spawn_mock_server(cloud_login_handler(account_status.clone())).await;
    cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");

    let refresh_hits = Arc::new(AtomicUsize::new(0));
    let refresh_hits_clone = refresh_hits.clone();
    let refresh_handler: MockHandler = Arc::new(move |request: MockRequest| {
        assert!(request.path == "/wunder/auth/refresh");
        refresh_hits_clone.fetch_add(1, Ordering::SeqCst);
        let presented = serde_json::from_str::<Value>(&request.body)
            .ok()
            .and_then(|body| body["refresh_token"].as_str().map(str::to_string));
        // Only the first, still-unrotated refresh token may be presented.
        assert_eq!(presented.as_deref(), Some(REFRESH_V1));
        MockResponse::json(
            200,
            refresh_body(REFRESH_V1, TOKEN_V2, REFRESH_V2, 90.0 * 24.0 * 3600.0),
        )
    });
    let server = spawn_mock_server(refresh_handler).await;
    let mut session = cloud.session().expect("session");
    session.server = server;
    cloud.install_session(session);

    let mut waiters = Vec::new();
    for _ in 0..8 {
        waiters.push({
            let cloud = cloud.clone();
            tokio::spawn(async move { cloud.recover_auth(TOKEN_V1).await })
        });
    }
    let mut recovered = 0;
    for waiter in waiters {
        if waiter.await.expect("task joins").expect("recover succeeds") {
            recovered += 1;
        }
    }
    assert_eq!(recovered, 8, "all waiters observe the rotated token");
    assert_eq!(
        refresh_hits.load(Ordering::SeqCst),
        1,
        "only one refresh request may leave the process"
    );
    cloud.logout(&store).await.expect("logout");
}

#[tokio::test]
async fn network_failure_marks_reconnecting_without_expiring() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));
    let account_status = Arc::new(AtomicUsize::new(0));
    let server = spawn_mock_server(cloud_login_handler(account_status.clone())).await;
    cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");

    // The refresh endpoint accepts the connection then drops it.
    let drop_handler: MockHandler = Arc::new(|request: MockRequest| {
        assert!(request.path == "/wunder/auth/refresh");
        MockResponse::connection_dropped()
    });
    let server = spawn_mock_server(drop_handler).await;
    let mut session = cloud.session().expect("session");
    session.server = server;
    cloud.install_session(session);

    let outcome = cloud.recover_auth(TOKEN_V1).await;
    assert!(outcome.is_err(), "network failure surfaces an error");
    let status = cloud.status();
    assert_eq!(status.connection, "reconnecting");
    assert!(!status.expired, "network failure must not mark expired");
    assert!(status.last_error.is_some());
    // The current token is kept for the next attempt.
    let persisted: Value = serde_json::from_str(
        &std::fs::read_to_string(home.path().join("config/cloud.session.json"))
            .expect("session file"),
    )
    .expect("session json");
    assert_eq!(persisted["token"], TOKEN_V1);
    assert_eq!(persisted["refresh_token"], REFRESH_V1);
    cloud.logout(&store).await.expect("logout");
}

#[tokio::test]
async fn heartbeat_posts_empty_log_batch() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));
    let account_status = Arc::new(AtomicUsize::new(0));
    let server = spawn_mock_server(cloud_login_handler(account_status.clone())).await;
    cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");

    let seen_bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = seen_bodies.clone();
    let heartbeat_handler: MockHandler = Arc::new(move |request: MockRequest| {
        assert!(request.path == "/wunder/cloud/logs");
        seen_clone
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(request.body.clone());
        MockResponse::json(200, json!({"data": {"accepted": 0, "last_seq": 0}}))
    });
    let server = spawn_mock_server(heartbeat_handler).await;
    let mut session = cloud.session().expect("session");
    session.server = server;
    cloud.install_session(session);

    cloud.send_heartbeat().await.expect("heartbeat succeeds");
    let bodies = seen_bodies
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    assert_eq!(bodies.len(), 1);
    let body: Value = serde_json::from_str(&bodies[0]).expect("heartbeat json");
    assert_eq!(body["logs"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["device_id"], TEST_DEVICE);
    assert_eq!(cloud.status().connection, "online");
    cloud.logout(&store).await.expect("logout");
}

#[tokio::test]
async fn proactive_renewal_only_fires_below_the_threshold() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home_far = tempfile::tempdir().expect("temp home");
    let home_near = tempfile::tempdir().expect("temp home");
    let refresh_hits = Arc::new(AtomicUsize::new(0));
    let refresh_hits_clone = refresh_hits.clone();
    let refresh_handler: MockHandler = Arc::new(move |request: MockRequest| {
        assert!(request.path == "/wunder/auth/refresh");
        refresh_hits_clone.fetch_add(1, Ordering::SeqCst);
        let presented = serde_json::from_str::<Value>(&request.body)
            .ok()
            .and_then(|body| body["refresh_token"].as_str().map(str::to_string));
        assert_eq!(presented.as_deref(), Some(REFRESH_V1));
        MockResponse::json(
            200,
            refresh_body(REFRESH_V1, TOKEN_V2, REFRESH_V2, 90.0 * 24.0 * 3600.0),
        )
    });
    let server = spawn_mock_server(refresh_handler).await;

    // Above the 72h threshold: no refresh request leaves the process.
    write_session_file(home_far.path(), TOKEN_V1, REFRESH_V1, 100.0 * 3600.0);
    cloud.set_session_base_dir(home_far.path().to_path_buf());
    let session = cloud.session().expect("far-future session loads");
    let mut session = session;
    session.server = server.clone();
    cloud.install_session(session);
    let renewed = cloud.renew_if_needed().await;
    assert!(!renewed);
    assert_eq!(refresh_hits.load(Ordering::SeqCst), 0);

    // Below the threshold: the keeper path refreshes and persists the pair.
    write_session_file(home_near.path(), TOKEN_V1, REFRESH_V1, 3600.0);
    cloud.set_session_base_dir(home_near.path().to_path_buf());
    let session = cloud.session().expect("near-expiry session loads");
    let mut session = session;
    session.server = server;
    cloud.install_session(session);
    let renewed = cloud.renew_if_needed().await;
    assert!(renewed, "token within the threshold is renewed");
    assert_eq!(refresh_hits.load(Ordering::SeqCst), 1);
    let persisted: Value = serde_json::from_str(
        &std::fs::read_to_string(home_near.path().join("config/cloud.session.json"))
            .expect("session file"),
    )
    .expect("session json");
    assert_eq!(persisted["token"], TOKEN_V2);
    assert_eq!(persisted["refresh_token"], REFRESH_V2);
    assert_eq!(cloud.status().connection, "online");
}
