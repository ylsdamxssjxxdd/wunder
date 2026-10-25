//! External UX backend experiments; no product entry point or frontend required.
#[path = "experience_probe/mock_model.rs"]
mod mock_model;

use anyhow::{bail, ensure, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wunder_desktop::{
    args::DesktopArgs, ModelEdit, NativeChatEvent, NativeChatInput, NativeDesktop, NativeStream,
};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Sample {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        case: String,
        #[arg(long, default_value_t = 1)]
        iterations: usize,
        #[arg(long, default_value_t = 32)]
        chunks: usize,
        #[arg(long, default_value_t = 8)]
        interval_ms: u64,
        #[arg(long, default_value_t = 50)]
        switches: usize,
        #[arg(long, default_value_t = 30)]
        soak_seconds: u64,
        #[arg(long, default_value_t = 2000)]
        files: usize,
    },
}

#[derive(Default)]
struct Measurements {
    values: BTreeMap<String, Vec<f64>>,
    checks: usize,
}
impl Measurements {
    fn add(&mut self, name: &str, value: f64) {
        let rows = self.values.entry(name.into()).or_default();
        // Fixed run budget. Never silently sample away measurements.
        assert!(rows.len() < 10000, "measurement budget exceeded");
        rows.push(value);
    }
    fn elapsed(&mut self, name: &str, start: Instant) {
        self.add(name, start.elapsed().as_secs_f64() * 1000.0);
    }
}
fn emit(value: Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}
fn handshake(expected: &str) -> Result<()> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    ensure!(line.trim() == expected, "invalid external handshake");
    Ok(())
}
fn isolated(root: &Path, reuse: bool) -> Result<()> {
    if reuse {
        ensure!(
            fs::read_to_string(root.join("fixture-owner.json"))?
                == r#"{"profile":"desktop-experience-v1"}"#,
            "recovery requires an owned synthetic fixture"
        );
    } else {
        fs::create_dir(root).context("experiment root must be new")?;
        fs::create_dir_all(root.join("home/config"))?;
        fs::write(
            root.join("fixture-owner.json"),
            r#"{"profile":"desktop-experience-v1"}"#,
        )?;
        fs::write(
            root.join("home/config/wunder.yaml"),
            r#"{"llm":{"default":"","models":{}},"channels":{"enabled":false},"lsp":{"enabled":false}}"#,
        )?;
        fs::write(
            root.join("home/config/desktop.settings.json"),
            r#"{"lan_mesh":{"enabled":false}}"#,
        )?;
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("WUNDER_") {
            std::env::remove_var(key);
        }
    }
    std::env::set_var("WUNDER_HOME", root.join("home"));
    std::env::set_var("WUNDER_STORAGE_DATA_ROOT", root.join("home/data"));
    Ok(())
}
fn start(root: &Path) -> Result<NativeDesktop> {
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(root.join("temp"));
    args.workspace = Some(root.join("workspace"));
    args.user = Some("fixture-user".into());
    NativeDesktop::start_with_args(args)
}
fn configure(desktop: &NativeDesktop, model: &mock_model::Model) -> Result<()> {
    desktop.save_model(ModelEdit {
        key: "fixture-model",
        provider: "openai",
        model: "fixture-model",
        base_url: &model.url,
        model_type: "llm",
        temperature: "0",
        timeout_s: "20",
        max_rounds: "4",
        max_context: "1048576",
        max_output: "8192",
        tool_call_mode: "tool_call",
        ..Default::default()
    })?;
    desktop.set_default_model("fixture-model")?;
    Ok(())
}
fn send(desktop: &NativeDesktop, session: &str, content: &str, id: &str) -> Result<NativeStream> {
    desktop.send_chat(NativeChatInput {
        session_id: session.into(),
        content: content.into(),
        client_message_id: Some(id.into()),
        reasoning_effort: None,
        attachments: Vec::new(),
    })
}
fn next(rt: &tokio::runtime::Runtime, stream: &mut NativeStream) -> Result<NativeChatEvent> {
    rt.block_on(async { tokio::time::timeout(Duration::from_secs(25), stream.recv()).await })?
        .context("stream closed before settlement")
}
fn delta(event: &Value) -> Option<&str> {
    let data = payload(event);
    let raw = event["event"].as_str().unwrap_or_default();
    let kind = if raw == "thread_item_delta" {
        data["source_event"].as_str().unwrap_or(raw)
    } else {
        raw
    };
    if kind == "llm_output_delta" {
        data["delta"].as_str()
    } else {
        None
    }
}
fn payload(event: &Value) -> &Value {
    event["data"].get("data").unwrap_or(&event["data"])
}
fn identity(event: &Value) -> bool {
    event["event"] == "native_turn_identity"
}
fn finish(rt: &tokio::runtime::Runtime, stream: &mut NativeStream) -> Result<()> {
    loop {
        match next(rt, stream)? {
            NativeChatEvent::Finished => return Ok(()),
            NativeChatEvent::Failed(_) => bail!("native execution failed"),
            _ => {}
        }
    }
}
fn first_delta(rt: &tokio::runtime::Runtime, stream: &mut NativeStream) -> Result<()> {
    loop {
        match next(rt, stream)? {
            NativeChatEvent::Event(event) if delta(&event).is_some() => return Ok(()),
            NativeChatEvent::Failed(_) | NativeChatEvent::Finished => {
                bail!("no live output before settlement")
            }
            _ => {}
        }
    }
}
fn verify_durable(desktop: &NativeDesktop, session: &str, text: &str) -> Result<()> {
    let rows = desktop.load_chat_headers(session)?;
    ensure!(
        rows.last()
            .is_some_and(|r| r.assistant.text
                == wunder_server::stable_core::thread_view::preview(text, 2048)),
        "chat preview differs from live text"
    );
    let page =
        desktop
            .state()
            .storage
            .thread_view_page(desktop.user_id(), session, "chat", None, 1)?;
    let item_id = &page
        .chat
        .last()
        .context("durable chat row missing")?
        .assistant
        .item_id;
    let item = desktop
        .state()
        .storage
        .get_thread_item(desktop.user_id(), session, item_id, false)?
        .context("durable assistant item missing")?;
    ensure!(
        item["payload"]["content"].as_str() == Some(text),
        "durable reply differs from live text"
    );
    Ok(())
}
fn run_stream(
    desktop: &NativeDesktop,
    rt: &tokio::runtime::Runtime,
    model: &mock_model::Model,
    session: &str,
    chunks: usize,
    id: &str,
    m: &mut Measurements,
    slow: bool,
) -> Result<()> {
    model.reset();
    let began = Instant::now();
    let mut stream = send(desktop, session, "fixture-stream", id)?;
    m.elapsed("submit_return_ms", began);
    let mut text = String::new();
    let mut previous = None;
    let mut final_seen = false;
    let mut received = 0;
    let mut first_received = None;
    let mut peak = 0;
    loop {
        peak = peak.max(stream.pending_events());
        match next(rt, &mut stream)? {
            NativeChatEvent::Event(event) => {
                if identity(&event) {
                    m.elapsed("accepted_ms", began);
                }
                if let Some(part) = delta(&event).filter(|s| !s.is_empty()) {
                    let now = Instant::now();
                    if received == 0 {
                        first_received = Some(now);
                        m.elapsed("first_delta_ms", began);
                    }
                    if let Some(last) = previous {
                        m.add(
                            "delta_gap_ms",
                            now.duration_since(last).as_secs_f64() * 1000.0,
                        );
                    }
                    previous = Some(now);
                    received += 1;
                    text.push_str(part);
                    if slow {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                if event["event"] == "llm_output" {
                    final_seen = true;
                }
            }
            NativeChatEvent::Finished => break,
            NativeChatEvent::Failed(_) => bail!("native execution failed"),
            NativeChatEvent::Queued => bail!("idle thread unexpectedly queued"),
        }
    }
    ensure!(
        text == mock_model::expected(chunks),
        "stream content loss, duplication or ordering error"
    );
    ensure!(
        received > 1 && final_seen,
        "stream was not delivered progressively"
    );
    ensure!(peak <= 128, "native stream exceeded bounded capacity");
    let trace = model.trace.lock().unwrap().clone();
    ensure!(
        trace.calls == 1 && trace.sent.len() == chunks,
        "unexpected provider request or chunk count"
    );
    m.add("pending_events_peak", peak as f64);
    if let Some(request) = trace.request {
        m.add(
            "provider_request_ms",
            request.duration_since(began).as_secs_f64() * 1000.0,
        );
    }
    if let Some(last) = trace.sent.last() {
        m.add(
            "settlement_after_source_ms",
            last.elapsed().as_secs_f64() * 1000.0,
        );
    }
    if let (Some(first), Some(last)) = (trace.sent.first(), trace.sent.last()) {
        m.add(
            "first_source_delivery_ms",
            first_received
                .context("missing first delta")?
                .duration_since(*first)
                .as_secs_f64()
                * 1000.0,
        );
        m.add(
            "source_span_ms",
            last.duration_since(*first).as_secs_f64() * 1000.0,
        );
    }
    // Chat headers intentionally contain a byte-bounded preview. Verify the
    // authoritative item separately; a preview is not a truncated stored reply.
    verify_durable(desktop, session, &text)?;
    ensure!(
        m.values.get("accepted_ms").is_some_and(|v| !v.is_empty()),
        "acceptance identity missing"
    );
    m.checks += 1;
    Ok(())
}
fn stop_resume(
    desktop: &NativeDesktop,
    rt: &tokio::runtime::Runtime,
    model: &mock_model::Model,
    session: &str,
    chunks: usize,
    m: &mut Measurements,
) -> Result<()> {
    let mut stream = send(desktop, session, "fixture-hold", "fixture-stop")?;
    first_delta(rt, &mut stream)?;
    let began = Instant::now();
    stream.cancel();
    finish(rt, &mut stream)?;
    m.elapsed("cancel_settled_ms", began);
    let turns = desktop
        .state()
        .storage
        .list_thread_turns(desktop.user_id(), session, None, 10)?;
    ensure!(
        turns
            .last()
            .is_some_and(|r| r["status"] == "cancelled" || r["status"] == "interrupted"),
        "cancel did not settle durably"
    );
    run_stream(
        desktop,
        rt,
        model,
        session,
        chunks,
        "fixture-resume",
        m,
        false,
    )?;
    m.checks += 1;
    Ok(())
}
fn queue(
    desktop: &NativeDesktop,
    rt: &tokio::runtime::Runtime,
    session: &str,
    chunks: usize,
    m: &mut Measurements,
) -> Result<()> {
    let mut running = send(desktop, session, "fixture-hold", "fixture-running")?;
    first_delta(rt, &mut running)?;
    let mut parked = Vec::new();
    for i in 0..3 {
        let began = Instant::now();
        let mut stream = send(
            desktop,
            session,
            "fixture-queued",
            &format!("fixture-queued-{i}"),
        )?;
        loop {
            match next(rt, &mut stream)? {
                NativeChatEvent::Queued => break,
                NativeChatEvent::Failed(_) | NativeChatEvent::Finished => {
                    bail!("queue admission failed")
                }
                _ => {}
            }
        }
        m.elapsed("queue_accept_ms", began);
        parked.push(stream);
    }
    let rows = desktop.list_queue_turns(session)?;
    ensure!(rows.len() == 3, "queue lost or duplicated submission");
    let ids = rows
        .iter()
        .rev()
        .map(|r| r.queue_id.clone())
        .collect::<Vec<_>>();
    let began = Instant::now();
    let reordered = desktop.reorder_queue_turns(session, &ids)?;
    m.elapsed("queue_reorder_ms", began);
    ensure!(
        reordered.iter().map(|r| &r.queue_id).eq(ids.iter()),
        "queue reorder mismatch"
    );
    // Interjection interrupts the executor and can dispatch immediately.
    // Withdraw the other rows first; never race withdrawal against dispatch.
    let selected_client = rows
        .iter()
        .find(|r| r.queue_id == ids[1])
        .context("selected queue row missing")?
        .client_message_id
        .clone();
    for row in rows.into_iter().filter(|r| r.queue_id != ids[1]) {
        let began = Instant::now();
        let content = desktop.withdraw_queue_turn(session, &row.queue_id)?;
        m.elapsed("queue_withdraw_ms", began);
        ensure!(
            content == "fixture-queued",
            "withdraw lost original content"
        );
    }
    ensure!(
        desktop.list_queue_turns(session)?.len() == 1,
        "queue withdrawal mismatch"
    );
    let began = Instant::now();
    desktop.interject_queue_turn(session, &ids[1])?;
    m.elapsed("queue_interject_ms", began);
    let mut selected_text = String::new();
    for (index, mut stream) in parked.into_iter().enumerate() {
        loop {
            match next(rt, &mut stream)? {
                NativeChatEvent::Finished => break,
                NativeChatEvent::Failed(_) => bail!("queued execution failed"),
                NativeChatEvent::Event(event) => {
                    if let Some(part) = delta(&event) {
                        ensure!(
                            selected_client == format!("fixture-queued-{index}"),
                            "withdrawn submission executed"
                        );
                        selected_text.push_str(part);
                    }
                    if event["event"] == "llm_output" {
                        ensure!(
                            selected_client == format!("fixture-queued-{index}"),
                            "withdrawn submission completed"
                        );
                        let final_text = payload(&event)["content"]
                            .as_str()
                            .context("queued final content missing")?;
                        ensure!(
                            final_text.starts_with(&selected_text),
                            "queued live/final content mismatch"
                        );
                        selected_text = final_text.into();
                    }
                }
                _ => {}
            }
        }
    }
    ensure!(
        selected_text == mock_model::expected(chunks),
        "interjected reply mismatch"
    );
    m.elapsed("queue_dispatch_complete_ms", began);
    finish(rt, &mut running)?;
    ensure!(
        desktop.list_queue_turns(session)?.is_empty(),
        "queue not empty after completion"
    );
    m.checks += 1;
    Ok(())
}
fn switch(
    desktop: &NativeDesktop,
    rt: &tokio::runtime::Runtime,
    sessions: &[String],
    count: usize,
    m: &mut Measurements,
) -> Result<()> {
    for index in 0..count {
        let session = &sessions[index % sessions.len()];
        let began = Instant::now();
        let snapshot = desktop.open_chat_snapshot(session)?;
        ensure!(
            snapshot.session.id == *session,
            "snapshot belongs to another thread"
        );
        let mut watch = desktop.watch_chat_with_initial_cursor(
            session,
            Some(snapshot.turns.clone()),
            Some(snapshot.change_cursor),
        );
        let update = rt
            .block_on(async { tokio::time::timeout(Duration::from_secs(5), watch.recv()).await })?;
        ensure!(
            matches!(update, Some(wunder_desktop::NativeThreadUpdate::Reset(ref turns)) if *turns == snapshot.turns),
            "initial observer snapshot missing"
        );
        drop(watch);
        desktop.close_session(session)?;
        m.elapsed("switch_cycle_ms", began);
        m.checks += 1;
    }
    Ok(())
}
fn workspace(
    desktop: &NativeDesktop,
    root: &Path,
    files: usize,
    m: &mut Measurements,
) -> Result<()> {
    let path = root.join("listing");
    let path = path.to_str().context("fixture path encoding")?;
    let mut names = std::collections::BTreeSet::new();
    for offset in [0, 200, 400] {
        let began = Instant::now();
        let page = desktop.workspace_local_listing(path, "", offset)?;
        m.elapsed("workspace_page_ms", began);
        ensure!(
            page.total == files as u64 && page.entries.len() == 200,
            "directory pagination mismatch"
        );
        ensure!(
            page.entries.windows(2).all(|w| w[0].name < w[1].name),
            "directory ordering mismatch"
        );
        for entry in &page.entries {
            ensure!(names.insert(entry.name.clone()), "directory pages overlap");
        }
        m.checks += 1;
    }
    let began = Instant::now();
    let refreshed = desktop.workspace_local_listing(path, "", 0)?;
    m.elapsed("workspace_refresh_ms", began);
    ensure!(
        refreshed.total == files as u64,
        "directory refresh mismatch"
    );
    Ok(())
}
fn sample(
    root: PathBuf,
    case: String,
    iterations: usize,
    chunks: usize,
    interval: u64,
    switches: usize,
    soak: u64,
    files: usize,
) -> Result<()> {
    ensure!(
        iterations == 1 && (2..=1024).contains(&chunks) && (1..=100).contains(&interval),
        "invalid stream budget"
    );
    ensure!(
        (1..=2000).contains(&switches)
            && (1..=7200).contains(&soak)
            && (600..=100000).contains(&files),
        "invalid workload budget"
    );
    let cases = [
        "startup",
        "recovery",
        "recovery_seed",
        "submit",
        "stream",
        "slow_consumer",
        "cancel_resume",
        "queue",
        "switch",
        "background",
        "workspace",
        "soak",
    ];
    ensure!(cases.contains(&case.as_str()), "unknown case");
    isolated(&root, case == "recovery")?;
    let root = root.canonicalize()?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let model = rt.block_on(mock_model::Model::start(chunks, interval))?;
    let startup = Instant::now();
    let desktop = if case == "startup" || case == "recovery" {
        None
    } else {
        Some(start(&root)?)
    };
    let startup_ms = startup.elapsed().as_secs_f64() * 1000.0;
    let mut sessions = Vec::new();
    if let Some(d) = &desktop {
        configure(d, &model)?;
        for _ in 0..4 {
            sessions.push(d.create_session(None)?.id);
        }
        if case == "switch" || case == "background" {
            for (i, session) in sessions.iter().enumerate() {
                run_stream(
                    d,
                    &rt,
                    &model,
                    session,
                    chunks,
                    &format!("fixture-seed-{i}"),
                    &mut Measurements::default(),
                    false,
                )?;
            }
        }
    }
    if case == "workspace" {
        fs::create_dir(root.join("listing"))?;
        for i in 0..files {
            fs::write(
                root.join("listing").join(format!("fixture-{i:06}.txt")),
                b"fixture",
            )?;
        }
    }
    emit(
        json!({"event":"ready","debug_assertions":cfg!(debug_assertions),"startup_ms":startup_ms}),
    );
    handshake("go")?;
    emit(json!({"event":"measurement_start"}));
    let blocking_before = wunder_server::runtime_metrics::snapshot().blocking;
    let began = Instant::now();
    let mut m = Measurements::default();
    if let Some(d) = &desktop {
        match case.as_str() {
            "submit" | "stream" | "slow_consumer" | "recovery_seed" => run_stream(
                d,
                &rt,
                &model,
                &sessions[0],
                chunks,
                "fixture-stream",
                &mut m,
                case == "slow_consumer",
            )?,
            "cancel_resume" => stop_resume(d, &rt, &model, &sessions[0], chunks, &mut m)?,
            "queue" => queue(d, &rt, &sessions[0], chunks, &mut m)?,
            "switch" => switch(d, &rt, &sessions, switches, &mut m)?,
            "background" => {
                let mut running = send(d, &sessions[0], "fixture-hold", "fixture-background")?;
                first_delta(&rt, &mut running)?;
                switch(d, &rt, &sessions[1..], switches, &mut m)?;
                ensure!(
                    d.state()
                        .storage
                        .list_thread_turns(d.user_id(), &sessions[0], None, 1)?
                        .first()
                        .is_some_and(|t| t["status"] == "running"),
                    "background thread stopped during switching"
                );
                running.cancel();
                finish(&rt, &mut running)?;
                m.checks += 1;
            }
            "workspace" => workspace(d, &root, files, &mut m)?,
            "soak" => {
                let end = Instant::now() + Duration::from_secs(soak);
                let mut cycle = 0;
                while Instant::now() < end {
                    ensure!(cycle < 10000, "soak cycle budget exceeded");
                    let mut batch = Measurements::default();
                    run_stream(
                        d,
                        &rt,
                        &model,
                        &sessions[cycle % 4],
                        chunks,
                        &format!("fixture-cycle-{cycle}"),
                        &mut batch,
                        false,
                    )?;
                    for (name, mut values) in batch.values {
                        if name == "delta_gap_ms" {
                            values.sort_by(f64::total_cmp);
                            m.add(
                                "cycle_delta_gap_p95_ms",
                                values[(values.len() * 95).div_ceil(100) - 1],
                            );
                            m.add("cycle_delta_gap_max_ms", *values.last().unwrap());
                        } else {
                            for value in values {
                                m.add(&name, value);
                            }
                        }
                    }
                    m.checks += batch.checks;
                    switch(d, &rt, &sessions, 1, &mut m)?;
                    cycle += 1;
                    std::thread::sleep(Duration::from_millis(1000));
                }
                m.add("soak_cycles", cycle as f64);
                m.add("soak_elapsed_ms", began.elapsed().as_secs_f64() * 1000.0);
            }
            _ => unreachable!(),
        }
    } else {
        let s = Instant::now();
        let d = start(&root)?;
        let rows = d.list_sessions(None)?;
        if case == "recovery" {
            let fixture: Value = serde_json::from_slice(&fs::read(root.join("recovery.json"))?)?;
            let session = fixture["session"]
                .as_str()
                .context("missing recovery session")?;
            ensure!(
                rows.iter().any(|r| r.id == session),
                "recovered session missing"
            );
            let snapshot = d.open_chat_snapshot(session)?;
            ensure!(!snapshot.turns.is_empty(), "recovered chat page empty");
            verify_durable(
                &d,
                session,
                fixture["text"].as_str().context("missing recovery text")?,
            )?;
            m.elapsed("recovery_ready_ms", s);
        } else {
            m.elapsed("startup_ready_ms", s);
            ensure!(rows.is_empty(), "fresh startup unexpectedly has history");
        }
        m.checks += 1;
        drop(d);
    }
    let elapsed = began.elapsed().as_secs_f64() * 1000.0;
    // Reuse production counters without adding a benchmark mode to the app.
    // The interval includes correctness reads and concurrent background work;
    // these totals locate expensive operations, not exclusive request spans.
    let mut blocking = wunder_server::runtime_metrics::snapshot()
        .blocking
        .into_iter()
        .filter_map(|after| {
            let before = blocking_before
                .iter()
                .find(|before| before.kind == after.kind && before.label == after.label);
            let calls = after.calls.saturating_sub(before.map_or(0, |b| b.calls));
            (calls > 0).then(|| {
                json!({
                    "kind": after.kind, "label": after.label, "calls": calls,
                    "total_queue_ms": (after.avg_queue_ms * after.calls as f64
                        - before.map_or(0.0, |b| b.avg_queue_ms * b.calls as f64)).max(0.0),
                    "total_exec_ms": (after.avg_exec_ms * after.calls as f64
                        - before.map_or(0.0, |b| b.avg_exec_ms * b.calls as f64)).max(0.0),
                })
            })
        })
        .collect::<Vec<_>>();
    ensure!(blocking.len() <= 256, "blocking diagnostic budget exceeded");
    blocking.sort_by(|a, b| {
        b["total_exec_ms"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["total_exec_ms"].as_f64().unwrap())
    });
    if case == "recovery_seed" {
        fs::write(
            root.join("recovery.json"),
            serde_json::to_vec(
                &json!({"session":sessions[0], "text":mock_model::expected(chunks)}),
            )?,
        )?;
    }
    emit(
        json!({"event":"result","samples_ms":[elapsed],"startup_ms":startup_ms,
        "check":{"assertions":m.checks,"metrics":m.values,"scope":"backend","model":"loopback-fixed-v1",
        "diagnostics":{"blocking":blocking}}}),
    );
    handshake("done")?;
    drop(desktop);
    drop(model);
    Ok(())
}
fn main() {
    let Command::Sample {
        root,
        case,
        iterations,
        chunks,
        interval_ms,
        switches,
        soak_seconds,
        files,
    } = Args::parse().command;
    if let Err(error) = sample(
        root,
        case,
        iterations,
        chunks,
        interval_ms,
        switches,
        soak_seconds,
        files,
    ) {
        emit(json!({"event":"error","reason":"experiment or correctness check failed"}));
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
