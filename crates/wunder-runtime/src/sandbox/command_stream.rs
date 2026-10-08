use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;

const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

pub(super) async fn request<F: FnMut(Value)>(
    client: &reqwest::Client,
    url: &str,
    payload: &Value,
    timeout: std::time::Duration,
    on_event: &mut F,
) -> Result<Option<Value>, String> {
    let response = client.post(url).timeout(timeout).json(payload).send().await;
    let response = match response {
        Ok(response) => response,
        Err(err) if err.is_connect() => return Ok(None),
        Err(_) => return Err("sandbox stream request interrupted".to_string()),
    };
    let status = response.status();
    // These responses explicitly indicate that no command handler was invoked.
    if matches!(status.as_u16(), 404 | 405) {
        return Ok(None);
    }
    if !status.is_success() {
        return Err("sandbox stream request rejected".to_string());
    }
    read_response(response.bytes_stream(), on_event)
        .await
        .map(Some)
}

// A successful HTTP response means the command may already be running. Any
// subsequent failure is terminal: retrying it could execute mutations twice.
pub(super) async fn read_response<S, E, F>(mut stream: S, on_event: &mut F) -> Result<Value, String>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
    F: FnMut(Value),
{
    let mut pending = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| format!("sandbox stream interrupted: {err}"))?;
        for fragment in chunk.split_inclusive(|byte| *byte == b'\n') {
            if pending.len().saturating_add(fragment.len()) > MAX_FRAME_BYTES {
                return Err("sandbox stream frame exceeds limit".to_string());
            }
            pending.extend_from_slice(fragment);
            if fragment.last() == Some(&b'\n') {
                if let Some(payload) = read_frame(&pending, on_event)? {
                    return Ok(payload);
                }
                pending.clear();
            }
        }
    }
    if let Some(payload) = read_frame(&pending, on_event)? {
        return Ok(payload);
    }
    Err("sandbox stream ended without final result".to_string())
}

fn read_frame<F: FnMut(Value)>(frame: &[u8], on_event: &mut F) -> Result<Option<Value>, String> {
    if frame.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let parsed: Value = serde_json::from_slice(frame)
        .map_err(|_| "sandbox stream contains invalid JSON".to_string())?;
    if parsed.get("type").and_then(Value::as_str) == Some("final") {
        return parsed
            .get("payload")
            .filter(|payload| payload.is_object())
            .cloned()
            .map(Some)
            .ok_or_else(|| "sandbox stream contains invalid final result".to_string());
    }
    on_event(parsed);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    async fn mock_server(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (format!("http://{address}/stream"), task)
    }

    #[tokio::test]
    async fn response_body_stall_times_out_and_is_not_safe_to_retry() {
        let router = axum::Router::new().route(
            "/stream",
            axum::routing::post(|| async {
                axum::body::Body::from_stream(
                    stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from_static(
                        b"{\"type\":\"command_start\"}\n",
                    ))])
                    .chain(stream::pending()),
                )
            }),
        );
        let (url, task) = mock_server(router).await;
        let mut events = Vec::new();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            request(
                &reqwest::Client::new(),
                &url,
                &json!({}),
                std::time::Duration::from_millis(200),
                &mut |event| events.push(event),
            ),
        )
        .await
        .unwrap();
        task.abort();
        assert!(result.is_err());
        assert_eq!(events, vec![json!({"type": "command_start"})]);
    }

    #[tokio::test]
    async fn only_unsupported_routes_allow_fallback() {
        for (status, safe) in [(404, true), (405, true), (500, false), (503, false)] {
            let router = axum::Router::new().route(
                "/stream",
                axum::routing::post(move || async move {
                    axum::http::StatusCode::from_u16(status).unwrap()
                }),
            );
            let (url, task) = mock_server(router).await;
            let result = request(
                &reqwest::Client::new(),
                &url,
                &json!({}),
                std::time::Duration::from_secs(2),
                &mut |_| {},
            )
            .await;
            task.abort();
            assert_eq!(result.ok(), if safe { Some(None) } else { None });
        }
    }

    #[tokio::test]
    async fn fragmented_stream_finishes_without_waiting_for_connection_close() {
        let chunks: Vec<Result<Bytes, &str>> = vec![
            Ok(Bytes::from_static(b"{\"type\":\"del")),
            Ok(Bytes::from_static(
                b"ta\",\"delta\":\"a\"}\n{\"type\":\"final\",\"payload\":{\"ok\":true}}\n",
            )),
        ];
        let stream = stream::iter(chunks).chain(stream::pending());
        let mut events = Vec::new();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            read_response(stream, &mut |event| events.push(event)),
        )
        .await
        .unwrap();
        assert_eq!(result, Ok(json!({"ok": true})));
        assert_eq!(events, vec![json!({"type": "delta", "delta": "a"})]);
    }

    #[tokio::test]
    async fn interrupted_or_missing_final_stream_fails() {
        for chunks in [
            vec![Err("interrupted")],
            vec![Ok(Bytes::from_static(b"{\"type\":\"delta\"}\n"))],
            vec![Ok(Bytes::from_static(
                b"{\"type\":\"final\",\"payload\":null}\n",
            ))],
            vec![Ok(Bytes::from_static(b"invalid\n"))],
        ] {
            assert!(read_response(stream::iter(chunks), &mut |_| {})
                .await
                .is_err());
        }
    }

    #[tokio::test]
    async fn accepts_final_without_newline_and_rejects_unbounded_frame() {
        let chunks: Vec<Result<Bytes, &str>> = vec![Ok(Bytes::from_static(
            b"{\"type\":\"final\",\"payload\":{\"ok\":false}}",
        ))];
        assert_eq!(
            read_response(stream::iter(chunks), &mut |_| {}).await,
            Ok(json!({"ok": false}))
        );
        let chunks: Vec<Result<Bytes, &str>> =
            vec![Ok(Bytes::from(vec![b'x'; MAX_FRAME_BYTES + 1]))];
        assert_eq!(
            read_response(stream::iter(chunks), &mut |_| {}).await,
            Err("sandbox stream frame exceeds limit".to_string())
        );
    }
}
