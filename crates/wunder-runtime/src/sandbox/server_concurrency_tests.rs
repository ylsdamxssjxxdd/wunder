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
