//! End-to-end contract for the desktop approval card: a turn gated by the
//! agent's `suggest` mode must surface its tool call to this window, stay
//! parked until the facade answers, and then execute it.
//!
//! A scripted OpenAI-compatible endpoint makes the plan deterministic: the
//! first request asks for one `write_file` call, the second answers with text.

use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wunder_desktop::{
    args::DesktopArgs, AgentSettingsEdit, NativeChatEvent, NativeChatInput, NativeDesktop,
    NativeStream, DEFAULT_AGENT_ID,
};

static RUNTIME_ENV: Mutex<()> = Mutex::new(());

/// Start the scripted model; it offers whatever `write_file` alias the request
/// carries, so the test never assumes a localized function name.
fn start_model_server() -> anyhow::Result<(tokio::runtime::Runtime, String, Arc<AtomicUsize>)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(request): Json<Value>| {
            let index = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                let (message, reason) = if index == 0 {
                    let name = request["tools"]
                        .as_array()
                        .and_then(|tools| {
                            tools.iter().find_map(|tool| {
                                let name = tool.pointer("/function/name")?.as_str()?;
                                (name == "write_file" || name == "写入文件")
                                    .then(|| name.to_string())
                            })
                        })
                        .expect("write_file must be offered");
                    (
                        json!({"role":"assistant","tool_calls":[{
                            "id":"fixture-call","type":"function",
                            "function":{"name":name,
                                "arguments":json!({"path":"temp/approval-e2e.txt","content":"approved"}).to_string()}
                        }]}),
                        "tool_calls",
                    )
                } else {
                    (
                        json!({"role":"assistant","content":"approved done"}),
                        "stop",
                    )
                };
                format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"delta":message,"finish_reason":reason}],
                        "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}})
                )
            }
        }),
    );
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))?;
    let address = listener.local_addr()?;
    runtime.spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((runtime, format!("http://{address}/v1"), calls))
}

fn isolated(base_url: &str) -> anyhow::Result<(
    std::sync::MutexGuard<'static, ()>,
    tempfile::TempDir,
    NativeDesktop,
)> {
    let guard = RUNTIME_ENV
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(
        config.join("wunder.yaml"),
        serde_json::to_vec(&serde_json::json!({
            "llm": {"default": "fixture-http", "models": {
                "fixture-http": {"enable": true, "provider": "openai",
                    "model": "fixture-http", "model_type": "llm",
                    "base_url": base_url, "api_key": "fixture",
                    "timeout_s": 30, "stream": true, "max_context": 16384,
                    "max_output": 1024, "support_vision": false,
                    "tool_call_mode": "function_call"}
            }},
            "tools": {"builtin": {"enabled": ["写入文件"]}},
            "skills": {"enabled": []}
        }))?,
    )?;
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "workspace_root": "", "desktop_token": "", "updated_at": 0,
            "lan_mesh": {"enabled": false}
        }))?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    std::env::set_var("WUNDER_HOME", directory.path().join("runtime"));
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    let desktop = NativeDesktop::start_with_args(args)?;
    Ok((guard, directory, desktop))
}

fn suggest_agent(desktop: &NativeDesktop) -> anyhow::Result<()> {
    desktop.update_agent_settings(
        DEFAULT_AGENT_ID,
        AgentSettingsEdit {
            name: "本地助手".into(),
            description: String::new(),
            system_prompt: "你是运行在本机的助手。".into(),
            model_name: "fixture-http".into(),
            icon_name: String::new(),
            icon_color: String::new(),
            icon: None,
            tool_names: vec!["写入文件".into()],
            preset_questions: Vec::new(),
            approval_mode: "suggest".into(),
            preview_skill: false,
            silent: false,
            prefer_mother: false,
        },
    )?;
    Ok(())
}

/// The gate stamps every approved execution with `meta.approval`; the runtime
/// wraps that payload in its event envelope, so walk the object instead of
/// guessing the exact nesting.
fn approval_status(value: &Value) -> Option<&str> {
    if let Some(status) = value
        .pointer("/meta/approval/status")
        .and_then(Value::as_str)
    {
        return Some(status);
    }
    value.get("data").and_then(approval_status)
}

fn find_artifact(root: &Path, name: &str) -> Option<PathBuf> {
    let mut budget = 20_000usize;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if budget == 0 {
                return None;
            }
            budget -= 1;
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if entry.file_name().to_string_lossy() == name {
                return Some(entry.path());
            }
        }
    }
    None
}

fn await_approval(stream: &mut NativeStream, deadline: Instant) -> anyhow::Result<String> {
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match stream.try_recv() {
            Ok(Some(NativeChatEvent::Event(value))) => {
                if value["event"].as_str() == Some("approval_request") {
                    let payload = &value["data"];
                    assert_eq!(payload["kind"], "patch", "write_file is a patch gate");
                    assert!(
                        payload["summary"]
                            .as_str()
                            .is_some_and(|summary| summary.contains("写入文件")),
                        "card summary names the tool: {payload}"
                    );
                    let id = payload["approval_id"].as_str().unwrap_or_default();
                    assert!(!id.is_empty(), "approval frame carries its id: {payload}");
                    return Ok(id.to_string());
                }
                seen.push(value["event"].as_str().unwrap_or("?").to_string());
            }
            Ok(Some(NativeChatEvent::Finished)) => break,
            Ok(Some(NativeChatEvent::Failed(error))) => {
                anyhow::bail!("turn failed before approval: {error}; events: {seen:?}")
            }
            Ok(Some(_)) => {}
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => anyhow::bail!("stream closed before approval: {error}; events: {seen:?}"),
        }
    }
    stream.cancel();
    anyhow::bail!("no approval request reached the window; events: {seen:?}")
}

/// Drain the rest of the turn. Returns `(settled, approved, denied)`.
fn await_settlement(
    stream: &mut NativeStream,
    deadline: Instant,
) -> anyhow::Result<(bool, bool, bool)> {
    let mut settled = false;
    let mut approved = false;
    let mut denied = false;
    while Instant::now() < deadline {
        match stream.try_recv() {
            Ok(Some(NativeChatEvent::Event(value))) => {
                if value["event"].as_str() == Some("tool_result") {
                    denied |= value.to_string().contains("Command not in allow list.");
                    approved |= approval_status(&value) == Some("approved");
                }
            }
            Ok(Some(NativeChatEvent::Finished)) | Ok(Some(NativeChatEvent::Failed(_))) => {
                settled = true;
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                settled = true;
                break;
            }
        }
    }
    Ok((settled, approved, denied))
}

fn start_suggest_turn(
    desktop: &NativeDesktop,
) -> anyhow::Result<(wunder_desktop::NativeSession, NativeStream)> {
    desktop.enable_tool_approvals();
    let session = desktop.create_session(None)?;
    let stream = desktop.send_chat(NativeChatInput {
        session_id: session.id.clone(),
        content: "写一份审批样例".into(),
        client_message_id: None,
        reasoning_effort: None,
        inquiry_turn_id: None,
        attachments: Vec::new(),
    })?;
    Ok((session, stream))
}

#[test]
fn suggest_turn_waits_for_the_card_and_runs_the_approved_tool() -> anyhow::Result<()> {
    let (_server, base_url, calls) = start_model_server()?;
    let (_guard, directory, desktop) = isolated(&base_url)?;
    suggest_agent(&desktop)?;
    assert_eq!(
        desktop.default_agent()?.approval_mode,
        "suggest",
        "the test agent must actually carry the gated mode"
    );
    let (session, mut stream) = start_suggest_turn(&desktop)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let approval_id = await_approval(&mut stream, deadline)?;
    // The gate is waiting now: no tool may have run yet.
    assert_eq!(
        find_artifact(directory.path(), "approval-e2e.txt"),
        None,
        "the gated write must not run before the answer"
    );

    desktop
        .decide_chat_approval(&session.id, &approval_id, "once")
        .expect("approval accepted");
    let (settled, approved, denied) = await_settlement(&mut stream, deadline)?;
    assert!(settled, "approved turn did not settle");
    assert!(!denied, "approved tool was still denied by the gate");
    assert!(approved, "tool result must record the once-scope approval");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one tool round followed by one final round"
    );
    let artifact = find_artifact(directory.path(), "approval-e2e.txt")
        .expect("the approved write must land on disk");
    assert_eq!(std::fs::read_to_string(artifact)?, "approved");
    Ok(())
}

#[test]
fn denied_turn_reports_the_denial_and_leaves_the_workspace_untouched() -> anyhow::Result<()> {
    let (_server, base_url, calls) = start_model_server()?;
    let (_guard, directory, desktop) = isolated(&base_url)?;
    suggest_agent(&desktop)?;
    let (session, mut stream) = start_suggest_turn(&desktop)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let approval_id = await_approval(&mut stream, deadline)?;

    desktop
        .decide_chat_approval(&session.id, &approval_id, "deny")
        .expect("denial accepted");
    let (settled, approved, denied) = await_settlement(&mut stream, deadline)?;
    assert!(settled, "denied turn did not settle");
    assert!(denied, "denied tool must report the gate refusal");
    assert!(!approved, "denied tool must not carry an approval stamp");
    assert_eq!(
        find_artifact(directory.path(), "approval-e2e.txt"),
        None,
        "denied write must not land on disk"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the model still gets its final round after the denial"
    );
    Ok(())
}
