//! Cron façade contract tests against an isolated SQLite runtime. These stay
//! independent of the chat streaming smoke so scheduler regressions surface
//! separately from orchestrator WIP.

use std::path::Path;
use wunder_desktop::{args::DesktopArgs, NativeCronJobEdit, NativeDesktop};

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

fn future_at(minutes: i64) -> String {
    (chrono::Local::now() + chrono::Duration::minutes(minutes))
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn base_edit() -> NativeCronJobEdit {
    NativeCronJobEdit {
        job_id: String::new(),
        name: "contract-job".into(),
        schedule_kind: "at".into(),
        schedule_at: future_at(5),
        every_s: 0,
        cron_expr: String::new(),
        timezone: String::new(),
        message: "契约测试消息".into(),
        agent_id: String::new(),
        delete_after_run: false,
        enabled: true,
    }
}

#[test]
fn cron_create_list_update_toggle_delete_roundtrip() {
    let runtime = start_isolated().expect("isolated runtime");
    let created = runtime.save_cron_job(&base_edit()).expect("create");
    assert!(!created.id.is_empty());
    assert!(created.enabled);
    assert_eq!(created.schedule_kind, "at");
    assert_eq!(created.message, "契约测试消息");
    assert!(runtime
        .list_cron_jobs()
        .expect("list")
        .iter()
        .any(|job| job.id == created.id));
    let fetched = runtime.get_cron_job(&created.id).expect("get");
    assert_eq!(fetched.name, "contract-job");

    // Validation rejections stay at the façade boundary.
    let past = NativeCronJobEdit {
        schedule_at: "2020-01-01 00:00".into(),
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&past).is_err(), "past time accepted");
    let empty_message = NativeCronJobEdit {
        message: "  ".into(),
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&empty_message).is_err());
    let empty_name = NativeCronJobEdit {
        name: String::new(),
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&empty_name).is_err());
    let bad_interval = NativeCronJobEdit {
        schedule_kind: "every".into(),
        every_s: 0,
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&bad_interval).is_err());
    let bad_cron = NativeCronJobEdit {
        schedule_kind: "cron".into(),
        schedule_at: String::new(),
        cron_expr: String::new(),
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&bad_cron).is_err());
    let unknown_kind = NativeCronJobEdit {
        schedule_kind: "yearly".into(),
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&unknown_kind).is_err());

    // Update keeps the same id; unknown ids are rejected.
    let updated = runtime
        .save_cron_job(&NativeCronJobEdit {
            job_id: created.id.clone(),
            name: "contract-updated".into(),
            enabled: false,
            ..base_edit()
        })
        .expect("update");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.name, "contract-updated");
    assert!(!updated.enabled);
    let missing = NativeCronJobEdit {
        job_id: "missing-cron-job".into(),
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&missing).is_err());

    runtime.toggle_cron_job(&created.id, true).expect("toggle");
    assert!(runtime.get_cron_job(&created.id).expect("get").enabled);

    // Run records stay empty until an execution settles; the listing itself
    // must work for an unknown id without panicking.
    assert!(runtime
        .list_cron_job_runs(&created.id)
        .expect("runs")
        .is_empty());
    assert!(runtime.list_cron_job_runs("missing-cron-job").is_err());

    runtime.delete_cron_job(&created.id).expect("delete");
    assert!(runtime.get_cron_job(&created.id).is_err());
    assert!(runtime.delete_cron_job(&created.id).is_err());
}

#[test]
fn cron_expression_normalizes_five_fields() {
    let runtime = start_isolated().expect("isolated runtime");
    let edit = NativeCronJobEdit {
        schedule_kind: "cron".into(),
        schedule_at: String::new(),
        cron_expr: "0 9 * * *".into(),
        name: "contract-cron".into(),
        ..base_edit()
    };
    let created = runtime.save_cron_job(&edit).expect("create cron");
    assert_eq!(created.schedule_kind, "cron");
    // The service stores the raw expression and normalizes it when computing
    // the next run, so the façade must preserve what the user wrote.
    assert_eq!(created.cron_expr, "0 9 * * *");
    runtime.delete_cron_job(&created.id).expect("delete");
}

#[test]
fn cron_every_rejects_out_of_range_interval() {
    let runtime = start_isolated().expect("isolated runtime");
    let too_small = NativeCronJobEdit {
        schedule_kind: "every".into(),
        every_s: 0,
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&too_small).is_err());
    let too_large = NativeCronJobEdit {
        schedule_kind: "every".into(),
        every_s: 200_000,
        ..base_edit()
    };
    assert!(runtime.save_cron_job(&too_large).is_err());
}

/// Minimal OpenAI-compatible SSE mock so the manual run settles deterministically.
fn start_mock_model() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let delta = "契约测试输出。";
            let body = format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{delta}\"}},\"finish_reason\":null}}]}}

                 data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}

                 data: [DONE]

"
            );
            let response = format!(
                "HTTP/1.1 200 OK
Content-Type: text/event-stream
Content-Length: {}
Connection: close

{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

#[test]
fn cron_manual_run_records_a_run_entry() {
    let port = start_mock_model();
    let directory = tempfile::tempdir().expect("tempdir");
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config).expect("config dir");
    std::fs::write(
        config.join("wunder.yaml"),
        "{}
",
    )
    .expect("wunder.yaml");
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": format!("http://127.0.0.1:{port}/v1"), "timeout_s": 10,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings).expect("settings"),
    )
    .expect("settings file");
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(
        directory
            .path()
            .join("runtime")
            .canonicalize()
            .expect("canonical"),
    );
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    let runtime = NativeDesktop::start_with_args(args).expect("isolated runtime");
    let edit = NativeCronJobEdit {
        name: "contract-run-now".into(),
        schedule_at: future_at(30),
        ..base_edit()
    };
    let created = runtime.save_cron_job(&edit).expect("create");
    let queued = runtime.run_cron_job_now(&created.id).expect("run now");
    assert!(queued == "queued" || queued == "running");
    let mut recorded = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(runs) = runtime.list_cron_job_runs(&created.id) {
            if let Some(first) = runs.first() {
                assert_eq!(first.trigger, "manual");
                assert!(!first.run_id.is_empty());
                recorded = true;
                break;
            }
        }
    }
    assert!(recorded, "manual run did not record within 20s");
    runtime.delete_cron_job(&created.id).expect("delete");
}

/// The mock model server doubles as the context probe target: it answers
/// GET /v1/models/<model> with an OpenAI-style context_length field.
#[test]
fn model_context_probe_reads_context_length() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let body = r#"{"data":[{"id":"test-model","context_length":32768}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    let directory = tempfile::tempdir().expect("tempdir");
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config).expect("config dir");
    std::fs::write(config.join("wunder.yaml"), "{}\n").expect("wunder.yaml");
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": format!("http://127.0.0.1:{port}/v1"), "timeout_s": 10,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings).expect("settings"),
    )
    .expect("settings file");
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(
        directory
            .path()
            .join("runtime")
            .canonicalize()
            .expect("canonical"),
    );
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    let runtime = NativeDesktop::start_with_args(args).expect("isolated runtime");

    let outcome = runtime
        .probe_model_context_window("test-model", None)
        .expect("probe");
    assert_eq!(outcome.max_context, Some(32768));
    assert!(outcome.message.contains("32768"));

    assert!(runtime
        .probe_model_context_window("missing-model", None)
        .is_err());
    // TTS voice probing rejects non-tts models at the façade boundary.
    assert!(runtime.probe_model_voices("test-model", None).is_err());
}
