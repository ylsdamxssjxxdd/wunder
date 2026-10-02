//! Channel façade contract tests against an isolated SQLite runtime. They pin
//! the secret-free projection (secrets are write-only) and the default binding
//! behaviour the web API provides, independent of the chat streaming smoke.

use std::path::Path;
use wunder_desktop::{
    args::DesktopArgs, NativeChannelAccountEdit, NativeChannelBindingEdit, NativeDesktop,
};

fn prepare_runtime(directory: &Path) -> std::io::Result<()> {
    let config = directory.join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "",
        "desktop_token": "",
        "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    Ok(())
}

fn start_isolated() -> Result<NativeDesktop, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    prepare_runtime(directory.path())?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    let runtime = NativeDesktop::start_with_args(args)?;
    // Keep the tempdir alive for the whole test by leaking it; the isolated
    // runtime owns open files inside it.
    std::mem::forget(directory);
    Ok(runtime)
}

const TEST_SECRET: &str = "contract-secret-value-do-not-leak";

fn feishu_edit() -> NativeChannelAccountEdit {
    NativeChannelAccountEdit {
        channel: "feishu".into(),
        account_name: "契约账号".into(),
        app_id: "cli_contract_app".into(),
        app_secret: TEST_SECRET.into(),
        enabled: true,
        ..Default::default()
    }
}

fn find_account<'a>(
    runtime: &'a NativeDesktop,
    account_id: &str,
) -> Option<wunder_desktop::ChannelAccountCard> {
    runtime
        .list_channel_accounts()
        .expect("list")
        .items
        .into_iter()
        .find(|item| item.account_id == account_id)
}

#[test]
fn channel_account_roundtrip_keeps_secrets_write_only() {
    let runtime = start_isolated().expect("isolated runtime");

    // Unsupported channels are rejected at the service boundary.
    let unsupported = NativeChannelAccountEdit {
        channel: "not-a-channel".into(),
        ..Default::default()
    };
    assert!(runtime.save_channel_account(&unsupported).is_err());

    let created = runtime.save_channel_account(&feishu_edit()).expect("create");
    assert!(!created.account_id.is_empty());
    assert!(created.active);
    assert!(created.configured);
    assert_eq!(created.channel, "feishu");
    assert_eq!(created.app_id, "cli_contract_app");
    assert!(created.secret_set);
    // The secret must not appear in any projected field.
    let dumped = format!("{created:?}");
    assert!(!dumped.contains(TEST_SECRET), "secret leaked through the card");

    let listed = runtime.list_channel_accounts().expect("list");
    assert!(listed.catalog.iter().any(|item| item.channel == "feishu"));
    assert!(listed.items.iter().any(|item| item.account_id == created.account_id));
    for item in &listed.items {
        assert!(!format!("{item:?}").contains(TEST_SECRET));
    }

    // The upsert synced a default wildcard binding.
    let bindings = runtime
        .list_channel_bindings(Some("feishu"), Some(&created.account_id))
        .expect("bindings");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].peer_id, "*");
    assert!(bindings[0].enabled);

    // Re-saving with an empty secret keeps the stored value and stays configured.
    let updated = runtime
        .save_channel_account(&NativeChannelAccountEdit {
            account_id: created.account_id.clone(),
            channel: "feishu".into(),
            account_name: "契约账号-改名".into(),
            ..Default::default()
        })
        .expect("update");
    assert_eq!(updated.account_id, created.account_id);
    assert_eq!(updated.name, "契约账号-改名");
    assert!(updated.configured, "empty secret lost the stored value");
    assert!(updated.secret_set);

    // Guided prefill values survive the round trip.
    assert_eq!(updated.app_id, "cli_contract_app");

    let logs = runtime
        .list_channel_runtime_logs(Some("feishu"), Some(&created.account_id), 50)
        .expect("logs");
    assert!(logs
        .iter()
        .any(|entry| entry.event == "account_upserted"));

    runtime
        .delete_channel_account("feishu", &created.account_id)
        .expect("delete");
    assert!(find_account(&runtime, &created.account_id).is_none());
    assert!(runtime
        .delete_channel_account("feishu", &created.account_id)
        .is_err());
    let bindings = runtime
        .list_channel_bindings(Some("feishu"), Some(&created.account_id))
        .expect("bindings");
    assert!(bindings.is_empty());
}

#[test]
fn channel_toggle_and_binding_roundtrip() {
    let runtime = start_isolated().expect("isolated runtime");
    let created = runtime.save_channel_account(&feishu_edit()).expect("create");

    runtime
        .toggle_channel_account("feishu", &created.account_id, false)
        .expect("disable");
    let disabled = find_account(&runtime, &created.account_id).expect("account");
    assert!(!disabled.active);
    assert_eq!(disabled.status, "disabled");
    assert!(disabled.configured, "toggle must not lose the configuration");

    runtime
        .toggle_channel_account("feishu", &created.account_id, true)
        .expect("enable");
    assert!(find_account(&runtime, &created.account_id)
        .expect("account")
        .active);

    // An extra peer binding for a specific user id.
    let binding = NativeChannelBindingEdit {
        channel: "feishu".into(),
        account_id: created.account_id.clone(),
        peer_kind: "user".into(),
        peer_id: "ou_contract_user".into(),
        agent_id: String::new(),
        enabled: true,
    };
    runtime.save_channel_binding(&binding).expect("bind");
    let bindings = runtime
        .list_channel_bindings(Some("feishu"), Some(&created.account_id))
        .expect("bindings");
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().any(|item| item.peer_id == "ou_contract_user"));

    runtime
        .delete_channel_binding("feishu", &created.account_id, "user", "ou_contract_user")
        .expect("unbind");
    let bindings = runtime
        .list_channel_bindings(Some("feishu"), Some(&created.account_id))
        .expect("bindings");
    assert_eq!(bindings.len(), 1);

    // Deleting a foreign or missing binding fails.
    assert!(runtime
        .delete_channel_binding("feishu", &created.account_id, "user", "ou_missing")
        .is_err());

    runtime
        .delete_channel_account("feishu", &created.account_id)
        .expect("delete");
}

#[test]
fn channel_config_json_path_supports_schema_less_channels() {
    let runtime = start_isolated().expect("isolated runtime");
    let edit = NativeChannelAccountEdit {
        channel: "qqbot".into(),
        account_name: "契约机器人".into(),
        config_json: r#"{"qqbot": {"app_id": "qq-app", "client_secret": "qq-secret", "token": "qq-token"}}"#.into(),
        enabled: true,
        ..Default::default()
    };
    let created = runtime.save_channel_account(&edit).expect("create");
    assert!(created.configured);
    assert!(created.secret_set);
    let dumped = format!("{created:?}");
    assert!(!dumped.contains("qq-secret"), "qqbot secret leaked");

    // Creating the same channel type again without account_id or JSON would
    // reuse the single existing account; a second explicit create needs a
    // config payload, which the service rejects when it is missing entirely.
    let missing_config = NativeChannelAccountEdit {
        channel: "qqbot".into(),
        enabled: true,
        ..Default::default()
    };
    // The single existing account is reused and keeps its configuration.
    let reused = runtime.save_channel_account(&missing_config).expect("reuse");
    assert_eq!(reused.account_id, created.account_id);
    assert!(reused.configured);

    runtime
        .delete_channel_account("qqbot", &created.account_id)
        .expect("delete");
}

#[test]
fn weixin_qr_wait_rejects_unknown_session_offline() {
    let runtime = start_isolated().expect("isolated runtime");
    let error = runtime
        .wait_weixin_qr_login("no-such-qr-session", 1_000)
        .expect_err("unknown session must fail");
    let text = error.to_string();
    assert!(
        text.contains("not found or expired"),
        "unexpected error text: {text}"
    );
}

#[test]
fn weixin_qr_png_renders_locally_without_network() {
    let runtime = start_isolated().expect("isolated runtime");
    let data_uri = runtime
        .weixin_qr_png_data_uri("wxqr_contract_payload")
        .expect("png data uri");
    assert!(data_uri.starts_with("data:image/png;base64,"));
    // A 29x29-class QR PNG encodes to several kilobytes of base64; an empty or
    // truncated render would stay far below that.
    assert!(data_uri.len() > 512, "png data uri suspiciously small");
}
