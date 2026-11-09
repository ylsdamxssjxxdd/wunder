//! A settled answer must reach the timeline whole.
//!
//! The chat view ships a byte-bounded preview plus a lazy body reference. The
//! desktop renders and copies the turn's text, so a preview that stops at
//! 2048 bytes used to cut a long answer mid-sentence (CJK text loses the most
//! characters per byte). This fixture seeds one long durable answer and checks
//! the thread-open projection returns it in full.
use std::sync::{Mutex, MutexGuard};

use serde_json::json;
use wunder_desktop::{args::DesktopArgs, NativeDesktop};
use wunder_server::stable_core::thread_store::*;

static RUNTIME_ENV: Mutex<()> = Mutex::new(());

fn isolated() -> anyhow::Result<(MutexGuard<'static, ()>, tempfile::TempDir, NativeDesktop)> {
    let guard = RUNTIME_ENV
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(
        config.join("wunder.yaml"),
        serde_json::to_vec(&json!({
            "llm": {"default": "", "models": {}},
            "skills": {"enabled": []}
        }))?,
    )?;
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&json!({
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

fn item(turn: &TurnId, id: String, payload: serde_json::Value) -> CanonicalRecord {
    CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
        item_id: ItemId::new(id).expect("generated identity"),
        turn_id: turn.clone(),
        root_turn_id: Some(turn.clone()),
        created_at_ms: 1_700_000_000_000,
        kind: CanonicalItemKind::AssistantMessage,
        payload,
        display_output: None,
        display_output_truncated: false,
    }))
}

/// One completed user round whose answer is longer than the view preview.
fn seed(desktop: &NativeDesktop, session: &str, answer: &str) -> anyhow::Result<()> {
    let state = desktop.state();
    let admitted = state.storage.accept_thread_turn(
        desktop.user_id(),
        session,
        &json!({"content": "fixture question", "client_message_id": "fixture-1"}),
    )?;
    let turn = TurnId::new(
        admitted["turn_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing admitted identity"))?
            .to_string(),
    )?;
    let thread = ThreadId::new(session.to_string())?;
    let rollout = RolloutId::new("fixture-rollout")?;
    let store = &state.thread_store;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(store.create_thread(CreateThreadRequest {
        metadata: ThreadMetadata {
            thread_id: thread.clone(),
            current_rollout_id: rollout.clone(),
            status: ThreadStatus::Active,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_000_000,
            title: Some("Fixture history".into()),
            workspace_id: None,
            pinned: false,
        },
        session_meta: SessionMeta {
            thread_id: thread.clone(),
            rollout_id: rollout.clone(),
            created_at_ms: 1_700_000_000_000,
            source: "fixture".into(),
            initial_system_prompt: "Synthetic fixture instructions.".into(),
            initial_memory: None,
            parent_thread_id: None,
            history_base: None,
            capabilities: json!({"synthetic": true}),
        },
    }))?;
    rt.block_on(store.begin_turn(desktop.user_id(), &thread, &turn, "fixture-model"))?;
    let payload = json!({
        "role": "assistant",
        "content": answer,
        "model_round": 1,
        "meta": {"type": "model_response"}
    });
    rt.block_on(store.append_canonical(
        &thread,
        vec![
            CanonicalRecord::ResponseItem(ResponseItem {
                item_id: ItemId::new(format!("{turn}:response-1"))?,
                turn_id: turn.clone(),
                root_turn_id: Some(turn.clone()),
                created_at_ms: 1_700_000_000_000,
                kind: CanonicalItemKind::AssistantMessage,
                payload: payload.clone(),
            }),
            item(&turn, format!("{turn}:text-1"), payload),
        ],
    ))?;
    Ok(())
}

#[test]
fn long_answer_keeps_every_byte_past_the_view_preview() -> anyhow::Result<()> {
    let (_guard, _directory, desktop) = isolated()?;
    let session = desktop.create_session(None)?;
    let answer = "长回复的每一段都必须完整显示。".repeat(120);
    assert!(
        answer.len() > 4096,
        "fixture answer must outgrow the preview"
    );
    seed(&desktop, &session.id, &answer)?;

    // The transport preview stays bounded; the row still names the whole body.
    let page = desktop.state().storage.thread_view_page(
        desktop.user_id(),
        &session.id,
        "chat",
        None,
        12,
    )?;
    let row = page.chat.first().expect("seeded turn");
    assert!(row.assistant.content.len() <= 2048);
    assert!(
        row.assistant.content.len() < answer.len(),
        "fixture must exercise the truncated preview"
    );
    assert_eq!(
        row.assistant.body.as_ref().map(|body| body.bytes),
        Some(answer.len())
    );

    let (turns, _, _) = desktop.load_chat_history_page(&session.id, i64::MAX)?;
    let turn = turns.first().expect("thread-open turn");
    assert_eq!(
        turn.assistant.text, answer,
        "the timeline must show the whole answer, not the preview head"
    );
    Ok(())
}
