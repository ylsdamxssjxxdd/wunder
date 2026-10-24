//! A bounded loopback provider. Only the model boundary is synthetic.
use anyhow::Result;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    routing::post,
    Json, Router,
};
use futures::stream;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Trace {
    pub request: Option<Instant>,
    pub sent: Vec<Instant>,
    pub calls: usize,
}

pub struct Model {
    pub url: String,
    pub trace: Arc<Mutex<Trace>>,
    task: tokio::task::JoinHandle<()>,
}

impl Model {
    pub async fn start(chunks: usize, interval_ms: u64) -> Result<Self> {
        let trace = Arc::new(Mutex::new(Trace::default()));
        let state = (trace.clone(), chunks, interval_ms);
        let app = Router::new()
            .route("/v1/chat/completions", post(completion))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let url = format!("http://{}/v1", listener.local_addr()?);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { url, trace, task })
    }

    pub fn reset(&self) {
        *self.trace.lock().unwrap() = Trace::default();
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn chunk(index: usize) -> String {
    format!("fixture-{index:04}-中🙂 ")
}
pub fn expected(chunks: usize) -> String {
    (0..chunks).map(chunk).collect()
}

async fn completion(
    State((trace, chunks, interval)): State<(Arc<Mutex<Trace>>, usize, u64)>,
    Json(request): Json<Value>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    {
        let mut t = trace.lock().unwrap();
        t.calls += 1;
        t.request = Some(Instant::now());
    }
    // The latest input chooses a long stream for cancellation/queue tests.
    let input = request["messages"]
        .as_array()
        .and_then(|messages| messages.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    let hold = input.contains("fixture-hold");
    let count = if hold { chunks.max(1000) } else { chunks };
    Sse::new(stream::unfold(
        (0usize, trace),
        move |(index, trace)| async move {
            if index > count + 1 {
                return None;
            }
            let event = if index < count {
                // Cancellation cases keep the provider open after the first
                // chunk. Switching speed must not decide when the job ends.
                if hold && index == 1 {
                    tokio::time::sleep(Duration::from_secs(120)).await;
                }
                tokio::time::sleep(Duration::from_millis(interval)).await;
                trace.lock().unwrap().sent.push(Instant::now());
                json!({"id":"fixture-completion","object":"chat.completion.chunk","model":"fixture-model",
                "choices":[{"index":0,"delta":{"content":chunk(index)},"finish_reason":null}]})
            } else if index == count {
                json!({"id":"fixture-completion","object":"chat.completion.chunk","model":"fixture-model",
                "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":64,"completion_tokens":count,"total_tokens":64+count}})
            } else {
                return Some((Ok(Event::default().data("[DONE]")), (index + 1, trace)));
            };
            Some((
                Ok(Event::default().data(event.to_string())),
                (index + 1, trace),
            ))
        },
    ))
}
