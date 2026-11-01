//! Chat image attachments must survive the full native round trip: the
//! composer's data URL is persisted into the private workspace, the durable
//! user message carries its public path, and the markdown image source
//! resolves back to decodable bytes. A break anywhere in that chain is what
//! leaves the bubble pasting the path instead of the thumbnail.

use std::sync::{Mutex, OnceLock};
use serde_json::Value;
use wunder_desktop::{args::DesktopArgs, NativeChatAttachment, NativeChatInput, NativeDesktop};

/// One runtime per home at a time: the second initializer in the same home
/// races the default-user seed.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Config, settings and the database all live under `WUNDER_HOME`; point it at
/// a throw-away directory so this test never touches the developer's real
/// home. The model endpoint is unreachable on purpose: the user message and
/// its attachments are durably appended before the first model call, so the
/// turn's failure does not affect what this test observes.
fn start_isolated() -> Result<(NativeDesktop, std::path::PathBuf), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir().expect("temp home");
    let home = directory.path().to_path_buf();
    std::env::set_var("WUNDER_HOME", &home);
    std::mem::forget(directory);
    let config = home.join("config");
    std::fs::create_dir_all(&config)?;
    let engine_config = serde_json::json!({
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false, "api_key": "contract-secret"}}}
    });
    std::fs::write(config.join("wunder.yaml"), serde_json::to_vec(&engine_config)?)?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(home.join("temp"));
    args.workspace = Some(home.join("workspace"));
    Ok((NativeDesktop::start_with_args(args)?, home))
}

/// A minimal valid 1×1 red PNG, as the composer would inline it.
fn png_bytes() -> Vec<u8> {
    [
        0x89_u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00,
        0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63,
        0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
    .to_vec()
}

fn png_data_url() -> String {
    use base64::Engine;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png_bytes())
    )
}

#[test]
fn image_attachment_round_trip_reaches_a_resolvable_public_path() {
    let _serial = serial();
    let (runtime, home) = start_isolated().expect("isolated runtime");

    let session = runtime.create_session(None).expect("create session");
    let session_id = session.id.clone();

    let mut stream = runtime
        .send_chat(NativeChatInput {
            session_id: session_id.clone(),
            content: "请看这张图".into(),
            client_message_id: Some("fixture-attach-1".into()),
            reasoning_effort: None,
            inquiry_turn_id: None,
            attachments: vec![NativeChatAttachment {
                name: "fixture.png".into(),
                content: png_data_url(),
                content_type: "image/png".into(),
            }],
        })
        .expect("send chat");
    // The turn ends (here: fails against the dead endpoint) once the stream
    // closes; the durable user message is already written by then.
    loop {
        match stream.blocking_recv() {
            Some(event) => {
                if let wunder_desktop::NativeChatEvent::Failed(error) = &event {
                    eprintln!("stream failed: {error}");
                }
                if let wunder_desktop::NativeChatEvent::Event(value) = &event {
                    let kind = value.get("event").and_then(Value::as_str).unwrap_or("");
                    if matches!(kind, "error" | "turn_terminal") {
                        eprintln!("event {kind}: {value}");
                    }
                }
            }
            None => break,
        }
    }

    let (_, turns) = runtime.get_session_turns(&session_id).expect("turns");
    let turn = turns
        .iter()
        .find(|turn| turn.user.text.contains("请看这张图"))
        .expect("user turn present");
    eprintln!("home: {}", home.display());
    eprintln!(
        "user attachments: {:?}",
        turn.user.attachments
    );
    assert!(
        !turn.user.attachments.is_empty(),
        "the durable user turn must project its attachments"
    );
    let public_path = turn.user.attachments[0].public_path.clone();
    assert!(
        !public_path.is_empty(),
        "a persisted image attachment carries a public path, got: {:?}",
        turn.user.attachments[0]
    );
    assert!(
        public_path.contains("chat_attachments/"),
        "the public path points at the attachment store: {public_path}"
    );
    // The frontend image classifier rejects `//`-prefixed sources as network
    // paths; a leaked Windows verbatim prefix would degrade the bubble back
    // into pasted path text.
    assert!(
        !public_path.starts_with("//"),
        "the public path must not carry a verbatim prefix: {public_path}"
    );

    // The bubble composes `![name](<public_path>)`; the desktop must resolve
    // that source back to bytes regardless of which workspace root the chat
    // page hands in (the active thread's root may be any folder).
    let bytes = runtime
        .workspace_image_bytes("", &public_path)
        .expect("image source resolves through the private workspace remap");
    assert_eq!(
        bytes, png_bytes(),
        "the resolved bytes are the persisted png"
    );
}
