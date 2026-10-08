//! Cloud channel engine tests: login/model synthesis lifecycle, 429 queue
//! retry with the same queue id, 401 session expiry and quota rejection.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wunder_server::cloud::{shared as cloud_shared, CloudQuotaInsufficient, CloudSessionExpired};
use wunder_server::config::LlmModelConfig;
use wunder_server::config_store::ConfigStore;
use wunder_server::llm::{build_llm_client, ChatMessage};

const TEST_TOKEN: &str = "wund_test_token";
const TEST_DEVICE: &str = "dev-test-1";

#[derive(Debug, Clone)]
struct MockRequest {
    #[allow(dead_code)]
    method: String,
    path: String,
    headers: HashMap<String, String>,
    #[allow(dead_code)]
    body: String,
}

#[derive(Debug, Clone)]
struct MockResponse {
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
}

type MockHandler = Arc<dyn Fn(MockRequest) -> MockResponse + Send + Sync>;

/// Minimal HTTP/1.1 mock: one request per connection, `connection: close`.
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
    let method = parts.next().unwrap_or("").to_string();
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
        method,
        path,
        headers,
        body,
    });
    let reason = match response.status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
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

fn recorder() -> (
    Arc<Mutex<Vec<MockRequest>>>,
    impl Fn(MockRequest) -> MockRequest,
) {
    let seen: Arc<Mutex<Vec<MockRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (seen, move |request| {
        sink.lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(request.clone());
        request
    })
}

/// Full wunder-server mock for the login flow (auth, device, account,
/// preferences, models, logs).
fn cloud_login_handler() -> MockHandler {
    Arc::new(move |request: MockRequest| match request.path.as_str() {
        "/wunder/auth/login" => MockResponse::json(
            200,
            json!({
                "data": {
                    "access_token": TEST_TOKEN,
                    "user": {"user_id": "u-test-1", "username": "tester"}
                }
            }),
        ),
        "/wunder/auth/me/preferences" => MockResponse::json(200, json!({"data": {}})),
        "/wunder/cloud/devices" => MockResponse::json(
            200,
            json!({"data": {"device_id": TEST_DEVICE, "max_concurrent_calls": 2}}),
        ),
        "/wunder/cloud/account" => MockResponse::json(
            200,
            json!({
                "data": {
                    "user_id": "u-test-1",
                    "username": "tester",
                    "quota": {
                        "balance": 100, "granted_total": 200, "used_total": 100,
                        "daily_grant": 50, "last_grant_date": "2026-10-08"
                    },
                    "concurrency": {"max_per_user": 2, "active": 0, "queued": 0},
                    "device": {"device_id": TEST_DEVICE, "revoked": false}
                }
            }),
        ),
        "/wunder/cloud/v1/models" => MockResponse::json(
            200,
            json!({
                "object": "list",
                "data": [
                    {"id": "m-alpha", "object": "model", "owned_by": "wunder"},
                    {"id": "m-beta", "object": "model", "owned_by": "wunder"}
                ]
            }),
        ),
        "/wunder/cloud/logs" => {
            MockResponse::json(200, json!({"data": {"accepted": 1, "last_seq": 0}}))
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
        api_key: Some(TEST_TOKEN.to_string()),
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

/// One shared cloud service per process; serialize every test that touches it.
static CLOUD_TEST_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn cloud_login_synthesizes_models_and_logout_cleans_up() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());
    let store = ConfigStore::new(home.path().join("config/wunder.yaml"));
    let server = spawn_mock_server(cloud_login_handler()).await;

    let status = cloud
        .login(&store, &server, "tester", "wund-test-pass", "desktop")
        .await
        .expect("login");
    assert!(status.logged_in);
    assert_eq!(status.username.as_deref(), Some("tester"));
    assert_eq!(status.user_id.as_deref(), Some("u-test-1"));
    assert_eq!(status.server.as_deref(), Some(server.as_str()));
    assert_eq!(status.device_id.as_deref(), Some(TEST_DEVICE));
    assert!(!status.expired);
    let quota = status.quota.expect("quota snapshot");
    assert_eq!(quota.balance, 100);
    assert_eq!(quota.daily_grant, 50);
    assert_eq!(status.max_concurrent_calls, Some(2));

    // Synthesized cloud models.
    let config = store.get().await;
    let alpha = config
        .llm
        .models
        .get("cloud/m-alpha")
        .expect("synthesized m-alpha");
    assert_eq!(alpha.provider.as_deref(), Some("wunder_cloud"));
    assert_eq!(
        alpha.base_url.as_deref(),
        Some(format!("{server}/wunder/cloud/v1").as_str())
    );
    assert_eq!(alpha.model.as_deref(), Some("m-alpha"));
    assert_eq!(alpha.api_key.as_deref(), Some(TEST_TOKEN));
    assert_eq!(alpha.model_type.as_deref(), Some("llm"));
    assert!(config.llm.models.contains_key("cloud/m-beta"));

    // Synthesis is idempotent: re-run leaves the same entries.
    let keys = cloud
        .synthesize_cloud_models(&store)
        .await
        .expect("re-synthesize");
    assert_eq!(keys, vec!["cloud/m-alpha", "cloud/m-beta"]);
    let config_after = store.get().await;
    assert_eq!(config.llm.models.len(), config_after.llm.models.len());
    assert_eq!(
        config_after
            .llm
            .models
            .get("cloud/m-alpha")
            .and_then(|model| model.api_key.clone()),
        Some(TEST_TOKEN.to_string())
    );

    // Session file exists under <base>/config/cloud.session.json.
    let session_path = home.path().join("config/cloud.session.json");
    assert!(session_path.exists());
    let raw = std::fs::read_to_string(&session_path).expect("session file");
    assert!(
        !raw.contains("password"),
        "session file must not hold secrets"
    );

    cloud.logout(&store).await.expect("logout");
    assert!(!cloud.status().logged_in);
    assert!(!session_path.exists());
    let config_after_logout = store.get().await;
    assert!(config_after_logout
        .llm
        .models
        .get("cloud/m-alpha")
        .is_none());
    assert!(config_after_logout.llm.models.get("cloud/m-beta").is_none());
}

#[tokio::test]
async fn cloud_queue_retry_reuses_queue_id_and_reports_position() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());

    let (seen, record) = recorder();
    let calls = Arc::new(AtomicUsize::new(0));
    let call_counter = calls.clone();
    let handler: MockHandler = Arc::new(move |request: MockRequest| {
        _ = record(request);
        let attempt = call_counter.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            return MockResponse::json(
                429,
                json!({
                    "ok": false,
                    "error": {
                        "code": "CLOUD_BUSY",
                        "message": "cloud busy",
                        "queue_id": "q-1",
                        "queue_ahead": 0,
                        "retry_after_ms": 30
                    }
                }),
            )
            .with_header("x-error-code", "CLOUD_BUSY")
            .with_header("x-wunder-queue-id", "q-1");
        }
        MockResponse::json(
            200,
            json!({
                "id": "chatcmpl-1",
                "choices": [{"message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
            }),
        )
        .with_header("x-wunder-request-id", "req-1")
        .with_header("x-wunder-quota-balance", "42")
        .with_header("x-wunder-quota-used", "8")
        .with_header("x-wunder-queue-started", "1")
    });
    let server = spawn_mock_server(handler).await;

    let positions: Arc<AtomicU64> = Arc::new(AtomicU64::new(u64::MAX));
    let observed = positions.clone();
    let client = build_llm_client(
        &cloud_llm_config(&format!("{server}/wunder/cloud/v1")),
        reqwest::Client::new(),
    )
    .with_cloud_queue_callback(Arc::new(move |position| {
        observed.store(position, Ordering::SeqCst);
    }));

    let response = client
        .complete(&[test_message()])
        .await
        .expect("queued call completes");
    assert_eq!(response.content, "pong");
    assert_eq!(
        positions.load(Ordering::SeqCst),
        0,
        "queue position observed"
    );

    let chat_requests: Vec<MockRequest> = seen
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .iter()
        .filter(|request| request.path == "/wunder/cloud/v1/chat/completions")
        .cloned()
        .collect();
    assert_eq!(chat_requests.len(), 2, "one retry with the queue id");
    assert!(!chat_requests[0].headers.contains_key("x-wunder-queue-id"));
    assert_eq!(
        chat_requests[1]
            .headers
            .get("x-wunder-queue-id")
            .map(String::as_str),
        Some("q-1"),
        "retry reuses the queue id"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cloud_session_401_maps_to_structured_error() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());

    let handler: MockHandler = Arc::new(|request: MockRequest| {
        assert!(request.path.ends_with("/chat/completions"));
        MockResponse::json(
            401,
            json!({"ok": false, "error": {"code": "SESSION_EXPIRED", "message": "session expired"}}),
        )
        .with_header("x-error-code", "SESSION_EXPIRED")
    });
    let server = spawn_mock_server(handler).await;
    let client = build_llm_client(
        &cloud_llm_config(&format!("{server}/wunder/cloud/v1")),
        reqwest::Client::new(),
    );

    let err = client
        .complete(&[test_message()])
        .await
        .expect_err("401 becomes an error");
    let expired = err
        .downcast_ref::<CloudSessionExpired>()
        .expect("structured CloudSessionExpired error");
    assert!(expired.to_string().contains("log in again"));
}

#[tokio::test]
async fn cloud_quota_insufficient_maps_to_structured_error() {
    let _guard = CLOUD_TEST_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let cloud = cloud_shared();
    let home = tempfile::tempdir().expect("temp home");
    cloud.set_session_base_dir(home.path().to_path_buf());

    let handler: MockHandler = Arc::new(|_request: MockRequest| {
        MockResponse::json(
            429,
            json!({
                "ok": false,
                "error": {
                    "code": "USER_QUOTA_INSUFFICIENT",
                    "message": "quota insufficient",
                    "quota_account": {
                        "quota": {"balance": 0, "granted_total": 10, "used_total": 10, "daily_grant": 5}
                    }
                }
            }),
        )
        .with_header("x-error-code", "USER_QUOTA_INSUFFICIENT")
    });
    let server = spawn_mock_server(handler).await;
    let client = build_llm_client(
        &cloud_llm_config(&format!("{server}/wunder/cloud/v1")),
        reqwest::Client::new(),
    );

    let err = client
        .complete(&[test_message()])
        .await
        .expect_err("quota rejection becomes an error");
    let quota = err
        .downcast_ref::<CloudQuotaInsufficient>()
        .expect("structured CloudQuotaInsufficient error");
    assert_eq!(quota.balance, 0);
    assert_eq!(quota.granted_total, 10);
    assert_eq!(quota.used_total, 10);
    assert_eq!(quota.daily_grant, 5);
    assert!(quota.to_string().contains("insufficient"));
}
