//! Real WebSocket subscription: handshake, bounded cursor delivery and terminal refusal.
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request, Response},
        Message,
    },
};
use wunder_server::cloud::{CloudService, CloudSessionFile, CloudThreadEvent};

#[tokio::test]
async fn watcher_subscribes_after_ready_and_stops_on_terminal_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(socket, |_: &Request, mut response: Response| {
            response
                .headers_mut()
                .insert("sec-websocket-protocol", "wunder".parse().unwrap());
            Ok(response)
        })
        .await
        .unwrap();
        let connect: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(connect["type"], "connect");
        socket
            .send(Message::Text(
                json!({"type":"ready","request_id":"cloud_connect","payload":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        loop {
            let frame: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            if frame["type"] == "ping" {
                continue;
            }
            assert_eq!(frame["type"], "watch");
            assert_eq!(frame["payload"]["view_seq"], 7);
            assert_eq!(frame["payload"]["session_id"], "thread_fixture");
            break;
        }
        for sequence in [8, 8, 9] {
            socket.send(Message::Text(json!({"type":"event","payload":{"event":"thread_view_change","data":{
                "view_seq":sequence,"epoch":1,"reset_required":false,"rows":[],"runtime_status":"idle","active_turn_id":""
            }}}).to_string().into())).await.unwrap();
        }
        socket
            .send(Message::Text(
                json!({"type":"error","payload":{"code":"SESSION_NOT_FOUND"}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    let home = tempfile::tempdir().unwrap();
    let session: CloudSessionFile = serde_json::from_value(json!({
        "server":format!("http://{address}"),"user_id":"fixture_user","username":"fixture_user",
        "scope":"local_desktop","token":"fixture_access","device_id":"fixture_device","device_name":"fixture",
        "client":"desktop","logged_in_at":1,"log_report":{"enabled":false}
    })).unwrap();
    session.save(home.path()).unwrap();
    let cloud = Arc::new(CloudService::new());
    cloud.set_client_kind("desktop");
    cloud.set_session_base_dir(home.path().into());
    let mut watch = cloud.watch_cloud_thread("thread_fixture".into(), 7);
    let mut sequences = Vec::new();
    let mut terminal = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = watch.recv().await {
            match event {
                CloudThreadEvent::Change(change) => sequences.push(change.view_seq),
                CloudThreadEvent::Connection(code) if code == "SESSION_NOT_FOUND" => {
                    terminal = true
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(sequences, vec![8, 9]);
    assert!(terminal);
    server.await.unwrap();
}
