//! External, headless chat-open benchmark. Never linked into the desktop UI.
//! Driven by scripts/desktop-bench/measure-chat-open.py.
use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{fs, io::Write, path::PathBuf, time::Instant};
use wunder_desktop::{args::DesktopArgs, NativeChatTurn, NativeDesktop};
use wunder_server::stable_core::thread_store::*;

const PROFILE: &str = "chat-open-actions-v1";

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Prepare {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 100)]
        user_rounds: u32,
        #[arg(long, default_value_t = 100)]
        actions: u32,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 1024)]
        text_bytes: usize,
        #[arg(long, default_value_t = 512)]
        reasoning_bytes: usize,
        #[arg(long, default_value_t = 4096)]
        tool_bytes: usize,
    },
    Sample {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "first_open")]
        case: String,
        /// Repeat the measured operation for sampling profilers. Each call is timed.
        #[arg(long, default_value_t = 1)]
        iterations: usize,
    },
}

fn emit(value: Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}

fn start(root: &std::path::Path) -> Result<NativeDesktop> {
    ensure!(
        root.join("fixture-owner.json").is_file(),
        "not a benchmark fixture"
    );
    // Environment is private to this single-purpose worker process. Never adopt
    // a caller's real desktop home or repository configuration.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("WUNDER_") {
            std::env::remove_var(key);
        }
    }
    std::env::set_var("WUNDER_HOME", root.join("home"));
    std::env::set_var("WUNDER_STORAGE_DATA_ROOT", root.join("home/data"));
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(root.join("temp"));
    args.workspace = Some(root.join("workspace"));
    args.user = Some("fixture-user".into());
    NativeDesktop::start_with_args(args)
}

// Fixed-size, seeded, non-repeating synthetic text. No files, commands, model
// providers or real user data participate in generation.
fn text(rng: &mut u64, bytes: usize) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789 ";
    (0..bytes)
        .map(|_| {
            *rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            alphabet[(*rng >> 32) as usize % alphabet.len()] as char
        })
        .collect()
}

fn item(turn: &TurnId, id: String, kind: CanonicalItemKind, payload: Value) -> CanonicalRecord {
    CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
        item_id: ItemId::new(id).expect("generated identity"),
        turn_id: turn.clone(),
        root_turn_id: Some(turn.clone()),
        created_at_ms: 1_700_000_000_000,
        kind,
        payload,
        display_output: None,
        display_output_truncated: false,
    }))
}

fn append_batch(
    rt: &tokio::runtime::Runtime,
    store: &wunder_server::storage::thread_store::LocalThreadStore,
    thread: &ThreadId,
    pending: &mut Vec<CanonicalRecord>,
) -> Result<()> {
    if !pending.is_empty() {
        rt.block_on(store.append_canonical(thread, std::mem::take(pending)))?;
    }
    Ok(())
}

fn prepare(root: PathBuf, rounds: u32, actions: u32, seed: u64, sizes: [usize; 3]) -> Result<()> {
    ensure!(
        (1..=2000).contains(&rounds) && (1..=2000).contains(&actions),
        "round count outside 1..=2000"
    );
    ensure!(
        u64::from(rounds) * u64::from(actions) <= 1_000_000,
        "fixture action budget exceeded"
    );
    ensure!(
        sizes.iter().all(|n| *n <= 64 * 1024) && sizes[0] > 0,
        "body size outside bounded fixture budget"
    );
    // Atomic directory creation prevents accidental reuse/overwrite of real data.
    fs::create_dir(&root).context("fixture directory must not already exist")?;
    fs::write(
        root.join("fixture-owner.json"),
        json!({"profile": PROFILE}).to_string(),
    )?;
    fs::create_dir_all(root.join("home/config"))?;
    fs::write(
        root.join("home/config/wunder.yaml"),
        "{\"llm\":{\"default\":\"\",\"models\":{}},\"channels\":{\"enabled\":false}}\n",
    )?;
    fs::write(
        root.join("home/config/desktop.settings.json"),
        "{\"lan_mesh\":{\"enabled\":false},\"llm\":{\"default\":\"\",\"models\":{}}}\n",
    )?;
    let root = root.canonicalize()?;
    let desktop = start(&root)?;
    let session = desktop.create_session(None)?;
    let state = desktop.state();
    let store = &state.thread_store;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let thread = ThreadId::new(session.id.clone())?;
    let rollout = RolloutId::new("fixture-rollout")?;
    let mut rng = seed;
    let tools = (actions - 1) / 3;
    let requests = actions - tools;
    let generated_at = Instant::now();
    for round in 1..=rounds {
        let prompt = format!("fixture input {round}: {}", text(&mut rng, 256));
        // Admit before creating the canonical thread: this establishes the
        // authoritative owner through the normal public storage contract.
        let admitted = state.storage.accept_thread_turn(
            desktop.user_id(),
            &session.id,
            &json!({"content": prompt, "client_message_id": format!("fixture-{round}")}),
        )?;
        let turn = TurnId::new(
            admitted["turn_id"]
                .as_str()
                .context("missing admitted identity")?,
        )?;
        if round == 1 {
            rt.block_on(store.create_thread(CreateThreadRequest {
                metadata: ThreadMetadata {
                    thread_id: thread.clone(),
                    current_rollout_id: rollout.clone(),
                    status: ThreadStatus::Active,
                    created_at_ms: 1_700_000_000_000,
                    updated_at_ms: 1_700_000_000_000,
                    title: Some("Fixture history".into()),
                    workspace_id: session.workspace_id.clone(),
                    pinned: false,
                },
                session_meta: SessionMeta {
                    thread_id: thread.clone(),
                    rollout_id: rollout.clone(),
                    created_at_ms: 1_700_000_000_000,
                    source: "benchmark-fixture".into(),
                    initial_system_prompt:
                        "Synthetic fixture instructions, frozen at creation.".into(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: json!({"synthetic":true,"profile":PROFILE,"seed":seed}),
                },
            }))?;
        }
        rt.block_on(store.begin_turn(desktop.user_id(), &thread, &turn, "fixture-model"))?;
        // Bound preparation batches by both records and bytes. The production
        // writer, durability and projector are used without per-item fsyncs.
        let mut pending = Vec::new();
        let mut pending_bytes = 0;
        for request in 1..=requests {
            let has_tool = request <= tools;
            let call = format!("{turn}:call-{request}");
            let mut payload = json!({
                "role":"assistant", "content": format!("fixture reply {round}/{request}: {}", text(&mut rng, sizes[0])),
                "reasoning_content":text(&mut rng, sizes[1]), "model_round":request,
                "meta":{"type":if has_tool {"tool_call"} else {"model_response"},
                    "message_stats":{"model_request_count":request,"tool_calls":request.min(tools),
                        "usage":{"input_tokens":256,"output_tokens":256,"total_tokens":512}}}
            });
            if has_tool {
                payload["tool_calls"] = json!([{"id":call,"type":"function",
                    "function":{"name":"fixture_tool","arguments":"{}"}}]);
            }
            let mut records = vec![
                CanonicalRecord::ResponseItem(ResponseItem {
                    item_id: ItemId::new(format!("{turn}:response-{request}"))?,
                    turn_id: turn.clone(),
                    root_turn_id: Some(turn.clone()),
                    created_at_ms: 1_700_000_000_000,
                    kind: CanonicalItemKind::AssistantMessage,
                    payload: payload.clone(),
                }),
                item(
                    &turn,
                    format!("{turn}:text-{request}"),
                    CanonicalItemKind::AssistantMessage,
                    payload,
                ),
                CanonicalRecord::Usage(UsageRecord {
                    turn_id: Some(turn.clone()),
                    input_tokens: 256,
                    output_tokens: 256,
                    metadata: json!({"model":"fixture-model","model_round":request,"estimated":true}),
                }),
            ];
            if has_tool {
                let result = text(&mut rng, sizes[2]);
                records.push(CanonicalRecord::ResponseItem(ResponseItem {
                    item_id: ItemId::new(format!("{turn}:result-{request}"))?,
                    turn_id: turn.clone(),
                    root_turn_id: Some(turn.clone()),
                    created_at_ms: 1_700_000_000_000,
                    kind: CanonicalItemKind::ToolResult,
                    payload: json!({"role":"tool","tool_call_id":call,"content":result}),
                }));
                records.push(item(
                    &turn,
                    format!("{turn}:tool-{request}"),
                    CanonicalItemKind::ToolResult,
                    json!({"tool":"fixture_tool","args":{},"ok":true,"data":{"text":result}}),
                ));
            }
            let bytes = serde_json::to_vec(&records)?.len();
            if pending.len() + records.len() > 96 || pending_bytes + bytes > 2 * 1024 * 1024 {
                append_batch(&rt, store, &thread, &mut pending)?;
                pending_bytes = 0;
            }
            pending.extend(records);
            pending_bytes += bytes;
        }
        append_batch(&rt, store, &thread, &mut pending)?;
        state.storage.update_thread_turn(
            desktop.user_id(),
            &session.id,
            turn.as_str(),
            "completed",
            "",
            &json!({}),
        )?;
        if round == rounds || round % 10 == 0 {
            emit(json!({"event":"progress","done":round,"total":rounds}));
        }
    }
    rt.block_on(store.shutdown(&thread))?;
    let (actual_rounds, actual_items) =
        state
            .storage
            .get_thread_log_counts(desktop.user_id(), &session.id, false)?;
    ensure!(
        actual_rounds == i64::from(rounds),
        "fixture user-round count mismatch"
    );
    ensure!(
        actual_items == i64::from(rounds) * i64::from(actions + 1),
        "fixture item count mismatch"
    );
    let manifest = json!({
        "profile":PROFILE,"session_id":session.id,"seed":seed,"user_rounds":rounds,
        "actions_per_round":actions,"model_requests_per_round":requests,"tool_actions_per_round":tools,
        "text_bytes":sizes[0],"reasoning_bytes":sizes[1],"tool_bytes":sizes[2],
        "actual_user_rounds":actual_rounds,"actual_items":actual_items,
        "generation_ms":generated_at.elapsed().as_secs_f64()*1000.0,
    });
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    emit(json!({"event":"prepared","fixture":manifest}));
    Ok(())
}

fn validate_turns(turns: &[NativeChatTurn], total: u64, expected_last: u64) -> Result<Value> {
    ensure!(
        turns.len() as u64 == expected_last.min(12),
        "history page size changed; rebaseline the measurement contract"
    );
    ensure!(
        turns.last().unwrap().user.user_round == expected_last as i64,
        "history tail mismatch"
    );
    ensure!(turns.len() as u64 <= total, "invalid history size");
    for turn in turns {
        ensure!(
            turn.user.text.starts_with("fixture input ")
                && turn.assistant.text.starts_with("fixture reply "),
            "missing expected conversation content"
        );
        ensure!(
            turn.rounds.is_empty(),
            "initial history unexpectedly materializes work-process rounds"
        );
    }
    ensure!(
        turns
            .windows(2)
            .all(|w| w[0].user.user_round + 1 == w[1].user.user_round),
        "history order or continuity mismatch"
    );
    Ok(
        json!({"turns":turns.len(),"first_round":turns[0].user.user_round,
        "last_round":expected_last,"text_bytes":turns.iter().map(|t|t.user.text.len()+t.assistant.text.len()).sum::<usize>()}),
    )
}

fn sample(root: PathBuf, case: String, iterations: usize) -> Result<()> {
    ensure!(
        (1..=10000).contains(&iterations),
        "iterations outside 1..=10000"
    );
    ensure!(
        case != "first_open" || iterations == 1,
        "first_open requires a new worker per sample"
    );
    let valid = [
        "first_open",
        "cached_reopen",
        "native_headers",
        "storage_page",
        "session_info",
        "change_cursor",
        "older_page",
    ];
    ensure!(valid.contains(&case.as_str()), "unknown benchmark case");
    let root = root.canonicalize()?;
    let manifest: Value = serde_json::from_slice(&fs::read(root.join("fixture.json"))?)?;
    ensure!(
        manifest["profile"] == PROFILE,
        "unsupported fixture profile"
    );
    let session = manifest["session_id"]
        .as_str()
        .context("missing fixture session")?;
    let rounds = manifest["user_rounds"]
        .as_u64()
        .context("missing fixture rounds")?;
    let startup = Instant::now();
    let desktop = start(&root)?;
    let startup_ms = startup.elapsed().as_secs_f64() * 1000.0;
    let mut older_before = None;
    if case == "cached_reopen" || case == "older_page" {
        let snapshot = desktop.open_chat_snapshot(session)?;
        validate_turns(&snapshot.turns, rounds, rounds)?;
        older_before = snapshot.history_before;
        if case == "older_page" {
            ensure!(
                snapshot.history_has_more,
                "fixture has no older history page"
            );
        }
    }
    emit(
        json!({"event":"ready","case":case,"startup_ms":startup_ms,"debug_assertions":cfg!(debug_assertions)}),
    );
    // A stdin handshake lets an external sampler/profiler attach after startup.
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    ensure!(line.trim() == "go", "expected external runner handshake");
    emit(json!({"event":"measurement_start","case":case}));
    let mut samples = Vec::with_capacity(iterations);
    let mut check = Value::Null;
    for _ in 0..iterations {
        let began = Instant::now();
        // Stop the clock before correctness checks or output serialization.
        let (elapsed, result) = match case.as_str() {
            "first_open" | "cached_reopen" => {
                let snapshot = desktop.open_chat_snapshot(session)?;
                let elapsed = began.elapsed();
                ensure!(
                    snapshot.session.id == session && snapshot.change_cursor > 0,
                    "snapshot identity/cursor mismatch"
                );
                let result = validate_turns(&snapshot.turns, rounds, rounds)?;
                ensure!(
                    snapshot.history_has_more == ((snapshot.turns.len() as u64) < rounds),
                    "history continuation mismatch"
                );
                (elapsed, result)
            }
            "native_headers" => {
                let turns = desktop.load_chat_headers(session)?;
                (began.elapsed(), validate_turns(&turns, rounds, rounds)?)
            }
            "storage_page" => {
                let page = desktop.state().storage.thread_view_page(
                    desktop.user_id(),
                    session,
                    "chat",
                    None,
                    12,
                )?;
                let elapsed = began.elapsed();
                ensure!(
                    page.chat.len() as u64 == rounds.min(12) && page.latest_round == rounds as i64,
                    "storage page mismatch"
                );
                ensure!(
                    page.chat.last().is_some_and(
                        |r| r.user_round == rounds as i64 && !r.assistant.content.is_empty()
                    ),
                    "storage tail mismatch"
                );
                (
                    elapsed,
                    json!({"turns":page.chat.len(),"last_round":page.latest_round}),
                )
            }
            "session_info" => {
                let info = desktop.get_session_info(session)?;
                let elapsed = began.elapsed();
                ensure!(info.id == session, "session mismatch");
                (elapsed, json!({"identity_valid":true}))
            }
            "change_cursor" => {
                let cursor = desktop
                    .state()
                    .storage
                    .latest_thread_change_seq_by_session(session)?;
                let elapsed = began.elapsed();
                ensure!(cursor > 0, "missing durable cursor");
                (elapsed, json!({"cursor":cursor}))
            }
            "older_page" => {
                let before = older_before.context("missing older page cursor")?;
                let (turns, _, _) = desktop.load_chat_history_page(session, before)?;
                (
                    began.elapsed(),
                    validate_turns(&turns, rounds, (before - 1) as u64)?,
                )
            }
            _ => unreachable!(),
        };
        samples.push(elapsed.as_secs_f64() * 1000.0);
        check = result;
    }
    emit(
        json!({"event":"result","case":case,"samples_ms":samples,"check":check,"startup_ms":startup_ms}),
    );
    // Keep counters available until the external sampler has read the final
    // resource values. Lease release and process teardown stay outside them.
    line.clear();
    std::io::stdin().read_line(&mut line)?;
    ensure!(
        line.trim() == "done",
        "expected sampler completion handshake"
    );
    desktop.close_session(session)?;
    Ok(())
}

fn main() {
    let result = match Args::parse().command {
        Command::Prepare {
            root,
            user_rounds,
            actions,
            seed,
            text_bytes,
            reasoning_bytes,
            tool_bytes,
        } => prepare(
            root,
            user_rounds,
            actions,
            seed,
            [text_bytes, reasoning_bytes, tool_bytes],
        ),
        Command::Sample {
            root,
            case,
            iterations,
        } => sample(root, case, iterations),
    };
    if result.is_err() {
        // Preserve a concrete stage/reason without copying paths, settings or
        // conversation bodies into permanent benchmark reports.
        emit(
            json!({"event":"error","reason":"backend operation or correctness validation failed"}),
        );
        if let Err(error) = result {
            eprintln!("{error:#}");
        }
        std::process::exit(1);
    }
}
