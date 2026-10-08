use super::*;
#[cfg(unix)]
use futures::StreamExt;

#[tokio::test]
async fn dropping_capture_task_closes_pipe_reader() {
    let (reader, mut writer) = tokio::io::duplex(32);
    let capture = CaptureTask(tokio::spawn(read_stream_capture(
        reader,
        STDOUT_CAPTURE_POLICY,
        None,
        "stdout",
    )));
    drop(capture);
    tokio::task::yield_now().await;
    use tokio::io::AsyncWriteExt;
    assert!(writer.write_all(b"value").await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn disconnected_stream_stops_running_command() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, build_router()).await.unwrap() });
    // exec keeps the child PID stable, allowing cancellation to be verified
    // without making assumptions about shell descendant cleanup.
    let request = json!({
        "user_id": "user", "tool": "执行命令", "workspace_root": dir.path(),
        "container_root": dir.path(), "allow_commands": ["*"],
        "args": {"content": "echo $$ > child.pid; exec sleep 30", "timeout_s": 30},
    });
    let response = reqwest::Client::new()
        .post(format!("http://{address}/sandboxes/execute_command_stream"))
        .json(&request)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream();
    assert!(stream.next().await.is_some());
    let pid_path = dir.path().join("child.pid");
    let pid = timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&pid_path).await {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(stream);
    let stopped = timeout(Duration::from_secs(3), async {
        loop {
            let running = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .await
                .unwrap()
                .success();
            if !running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    server.abort();
    assert!(stopped.is_ok(), "command must stop after client disconnect");
}

#[tokio::test]
async fn background_command_session_can_be_polled_only_in_its_scope() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, build_router()).await.unwrap() });
    let client = reqwest::Client::new();
    let command = if cfg!(windows) {
        "cmd.exe /C echo background-ready"
    } else {
        "printf background-ready"
    };
    let launch = client
        .post(format!(
            "http://{address}/sandboxes/command-sessions/launch"
        ))
        .json(&json!({
            "user_id": "user_a", "session_id": "thread_a", "workspace_root": "/",
            "container_root": "/", "allow_commands": ["*"],
            "args": {"content": command, "yield_time_ms": 50}
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        launch.get("ok").and_then(Value::as_bool),
        Some(true),
        "launch response: {launch}"
    );
    let command_session_id = launch
        .pointer("/data/command_session_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let poll = client
        .post(format!("http://{address}/sandboxes/command-sessions/poll"))
        .json(&json!({
            "user_id": "user_a", "session_id": "thread_a",
            "command_session_id": command_session_id, "yield_time_ms": 500
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        poll.pointer("/data/status").and_then(Value::as_str),
        Some("exited")
    );
    assert!(poll
        .pointer("/data/stdout")
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains("background-ready")));
    let foreign = client
        .post(format!("http://{address}/sandboxes/command-sessions/poll"))
        .json(&json!({
            "user_id": "user_b", "session_id": "thread_a",
            "command_session_id": poll.pointer("/data/command_session_id").cloned().unwrap_or(Value::Null)
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    server.abort();
    assert_eq!(foreign.get("ok").and_then(Value::as_bool), Some(false));
}

#[tokio::test]
async fn background_command_session_can_be_cancelled() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, build_router()).await.unwrap() });
    let client = reqwest::Client::new();
    let command = if cfg!(windows) {
        "ping 127.0.0.1 -n 30 > NUL"
    } else {
        "sleep 30"
    };
    let launch = client
        .post(format!(
            "http://{address}/sandboxes/command-sessions/launch"
        ))
        .json(&json!({
            "user_id": "user_cancel", "session_id": "thread_cancel",
            "workspace_root": "/", "container_root": "/", "allow_commands": ["*"],
            "args": {"content": command, "yield_time_ms": 50}
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(launch.get("ok").and_then(Value::as_bool), Some(true));
    let id = launch
        .pointer("/data/command_session_id")
        .and_then(Value::as_str)
        .unwrap();
    let cancel = client
        .post(format!(
            "http://{address}/sandboxes/command-sessions/cancel"
        ))
        .json(&json!({
            "user_id": "user_cancel", "session_id": "thread_cancel",
            "command_session_id": id
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(cancel.get("ok").and_then(Value::as_bool), Some(true));
    let poll = client
        .post(format!("http://{address}/sandboxes/command-sessions/poll"))
        .json(&json!({
            "user_id": "user_cancel", "session_id": "thread_cancel",
            "command_session_id": id, "yield_time_ms": 500
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        poll.pointer("/data/status").and_then(Value::as_str),
        Some("exited")
    );
    server.abort();
}
