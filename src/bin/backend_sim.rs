use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use uuid::Uuid;
use wunder_server::config_store::ConfigStore;
use wunder_server::schemas::{StreamEvent, WunderRequest};
use wunder_server::state::AppState;

const DEFAULT_REQUESTS: usize = 120;
const DEFAULT_CONCURRENCY: usize = 24;
const DEFAULT_REPORT_PATH: &str = "temp_dir/backend_sim_report.json";
const DEFAULT_BASELINE_PATH: &str = "temp_dir/backend_sim_baseline.json";
const MOCK_MODEL_NAME: &str = "__sim_mock__";
const MAX_FAILURE_SAMPLES: usize = 20;
const MAX_PROGRESS_UPDATES: usize = 20;

#[tokio::main]
async fn main() -> Result<()> {
    let args = SimArgs::parse_from_env(env::args().skip(1).collect())?;
    let run_id = Uuid::new_v4().simple().to_string();
    let runtime_dir = PathBuf::from("temp_dir/backend_sim/runtime").join(&run_id);
    let config_store = ConfigStore::new(ConfigStore::override_path_default());
    let mut config = config_store.get().await;
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = runtime_dir
        .join("backend_sim.sqlite3")
        .to_string_lossy()
        .to_string();
    config.cron.enabled = false;
    config.agent_queue.enabled = false;
    config.channels.enabled = false;
    config.gateway.enabled = false;
    config.workspace.root = runtime_dir.join("workspaces").to_string_lossy().to_string();
    println!(
        "[backend_sim] run_id={run_id} runtime_dir={}",
        runtime_dir.to_string_lossy()
    );
    let state = Arc::new(AppState::new(config_store, config)?);

    let started_at = Utc::now();
    let started = Instant::now();
    let outcomes = run_simulation(Arc::clone(&state), &args).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    let mut report = build_report(&args, started_at, elapsed_ms, outcomes);

    if let Some(baseline_path) = args.baseline.as_ref().filter(|_| !args.write_baseline) {
        let baseline = read_json::<BaselineFile>(baseline_path).with_context(|| {
            format!(
                "failed to load baseline file: {}",
                baseline_path.to_string_lossy()
            )
        })?;
        let drift = compare_with_baseline(&baseline.signature, &report, &args);
        print_drift(&drift);
        report.drift = Some(drift.clone());

        if args.fail_on_drift && !drift.passed {
            if let Some(report_path) = &args.report {
                write_json(report_path, &report)?;
            }
            return Err(anyhow!("drift check failed"));
        }
    }

    if let Some(report_path) = &args.report {
        write_json(report_path, &report)?;
        println!(
            "[backend_sim] report written to {}",
            report_path.to_string_lossy()
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    if args.write_baseline {
        let baseline = BaselineFile {
            generated_at: Utc::now(),
            signature: BaselineSignature::from_report(&report),
        };
        let baseline_path = args
            .baseline
            .as_ref()
            .ok_or_else(|| anyhow!("baseline path is missing"))?;
        write_json(baseline_path, &baseline)?;
        println!(
            "[backend_sim] baseline written to {}",
            baseline_path.to_string_lossy()
        );
    }

    print_summary(&report);
    std::mem::forget(state);
    Ok(())
}

async fn run_simulation(state: Arc<AppState>, args: &SimArgs) -> Vec<RequestOutcome> {
    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut tasks = FuturesUnordered::new();
    let args = Arc::new(args.clone());
    let shared_session_id = args.shared_session_id();
    let config_overrides = Arc::new(sim_config_overrides());

    for request_index in 0..args.requests {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should remain available");
        let state = Arc::clone(&state);
        let args = Arc::clone(&args);
        let shared_session_id = shared_session_id.clone();
        let config_overrides = Arc::clone(&config_overrides);
        tasks.push(tokio::spawn(async move {
            let _permit = permit;
            run_one_request(
                state,
                args,
                request_index,
                shared_session_id,
                config_overrides,
            )
            .await
        }));
    }

    let progress_step = (args.requests / MAX_PROGRESS_UPDATES).max(1);
    let mut completed = 0usize;
    let mut outcomes = Vec::with_capacity(args.requests);

    while let Some(joined) = tasks.next().await {
        completed += 1;
        let outcome = match joined {
            Ok(outcome) => outcome,
            Err(err) => RequestOutcome::panic_failure(completed.saturating_sub(1), err.to_string()),
        };
        if completed.is_multiple_of(progress_step) || completed == args.requests {
            println!("[backend_sim] progress {completed}/{}", args.requests);
        }
        outcomes.push(outcome);
    }

    outcomes.sort_by_key(|item| item.request_index);
    outcomes
}

async fn run_one_request(
    state: Arc<AppState>,
    args: Arc<SimArgs>,
    request_index: usize,
    shared_session_id: Option<String>,
    config_overrides: Arc<Value>,
) -> RequestOutcome {
    let user_id = format!(
        "{}_{}",
        args.user_prefix,
        request_index % args.concurrency.max(1)
    );
    let session_id = args.resolve_session_id(request_index, shared_session_id.as_deref());
    let request = WunderRequest {
        user_id: user_id.clone(),
        question: args.question.clone(),
        tool_names: Vec::new(),
        skip_tool_calls: args.skip_tool_calls,
        stream: args.stream,
        debug_payload: false,
        session_id: Some(session_id.clone()),
        agent_id: None,
        model_name: Some(MOCK_MODEL_NAME.to_string()),
        language: Some("zh-CN".to_string()),
        config_overrides: Some(config_overrides.as_ref().clone()),
        agent_prompt: None,
        attachments: None,
        allow_queue: true,
        is_admin: false,
    };

    if args.stream {
        run_stream_request(state, request_index, user_id, session_id, request).await
    } else {
        run_single_request(state, request_index, user_id, session_id, request).await
    }
}
async fn run_single_request(
    state: Arc<AppState>,
    request_index: usize,
    user_id: String,
    session_id: String,
    request: WunderRequest,
) -> RequestOutcome {
    let started = Instant::now();
    match state.orchestrator.run(request).await {
        Ok(response) => {
            let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            RequestOutcome {
                request_index,
                user_id,
                session_id,
                ok: true,
                latency_ms,
                first_event_latency_ms: None,
                event_count: 0,
                final_event_seen: true,
                event_id_monotonic: true,
                stop_reason: response.stop_reason,
                answer_digest: Some(digest_text(&response.answer)),
                event_digest: None,
                error_kind: None,
                error_message: None,
            }
        }
        Err(err) => {
            let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let text = err.to_string();
            RequestOutcome {
                request_index,
                user_id,
                session_id,
                ok: false,
                latency_ms,
                first_event_latency_ms: None,
                event_count: 0,
                final_event_seen: false,
                event_id_monotonic: true,
                stop_reason: None,
                answer_digest: None,
                event_digest: None,
                error_kind: Some(classify_error(&text)),
                error_message: Some(text),
            }
        }
    }
}

async fn run_stream_request(
    state: Arc<AppState>,
    request_index: usize,
    user_id: String,
    session_id: String,
    request: WunderRequest,
) -> RequestOutcome {
    let started = Instant::now();
    let stream = state.orchestrator.stream(request).await;
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let text = err.to_string();
            return RequestOutcome {
                request_index,
                user_id,
                session_id,
                ok: false,
                latency_ms,
                first_event_latency_ms: None,
                event_count: 0,
                final_event_seen: false,
                event_id_monotonic: true,
                stop_reason: None,
                answer_digest: None,
                event_digest: None,
                error_kind: Some(classify_error(&text)),
                error_message: Some(text),
            };
        }
    };

    let mut first_event_latency_ms = None;
    let mut event_count = 0usize;
    let mut final_event_seen = false;
    let mut event_id_monotonic = true;
    let mut last_event_id = None::<i64>;
    let mut stop_reason = None::<String>;
    let mut final_answer = None::<String>;
    let mut error_kind = None::<String>;
    let mut error_message = None::<String>;
    let mut event_hasher = Sha256::new();

    while let Some(next_event) = stream.next().await {
        let event = match next_event {
            Ok(event) => event,
            Err(infallible) => match infallible {},
        };

        if first_event_latency_ms.is_none() {
            first_event_latency_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        }
        event_count += 1;

        event_hasher.update(event.event.as_bytes());
        event_hasher.update(b"|");

        if let Some(event_id) = event
            .id
            .as_deref()
            .and_then(|value| value.parse::<i64>().ok())
        {
            if let Some(last) = last_event_id {
                if event_id <= last {
                    event_id_monotonic = false;
                }
            }
            last_event_id = Some(event_id);
        }

        let payload = event_payload(&event);
        if event.event == "final" {
            final_event_seen = true;
            stop_reason = payload
                .get("stop_reason")
                .and_then(Value::as_str)
                .map(str::to_string);
            final_answer = payload
                .get("answer")
                .and_then(Value::as_str)
                .map(str::to_string);
        }

        if event.event == "error" {
            error_kind = payload
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some("STREAM_ERROR".to_string()));
            error_message = payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some(payload.to_string()));
        }
    }

    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let event_digest = if event_count > 0 {
        Some(short_hash_hex(&event_hasher.finalize()))
    } else {
        None
    };

    let mut ok = final_event_seen && error_kind.is_none();
    if !final_event_seen && error_kind.is_none() {
        error_kind = Some("MISSING_FINAL".to_string());
        error_message = Some("stream ended without final event".to_string());
        ok = false;
    }

    RequestOutcome {
        request_index,
        user_id,
        session_id,
        ok,
        latency_ms,
        first_event_latency_ms,
        event_count,
        final_event_seen,
        event_id_monotonic,
        stop_reason,
        answer_digest: final_answer.as_deref().map(digest_text),
        event_digest,
        error_kind,
        error_message,
    }
}

fn event_payload(event: &StreamEvent) -> &Value {
    event.data.get("data").unwrap_or(&event.data)
}

fn build_report(
    args: &SimArgs,
    generated_at: DateTime<Utc>,
    elapsed_ms: f64,
    outcomes: Vec<RequestOutcome>,
) -> SimulationReport {
    let latencies: Vec<f64> = outcomes.iter().map(|item| item.latency_ms).collect();
    let first_event_latencies: Vec<f64> = outcomes
        .iter()
        .filter_map(|item| item.first_event_latency_ms)
        .collect();
    let event_counts: Vec<f64> = outcomes
        .iter()
        .map(|item| item.event_count as f64)
        .collect();

    let success = outcomes.iter().filter(|item| item.ok).count();
    let failed = outcomes.len().saturating_sub(success);
    let final_missing = outcomes
        .iter()
        .filter(|item| !item.final_event_seen)
        .count();
    let monotonic_violation = outcomes
        .iter()
        .filter(|item| !item.event_id_monotonic)
        .count();

    let stop_reasons = count_by_string(outcomes.iter().filter_map(|item| item.stop_reason.clone()));
    let error_kinds = count_by_string(outcomes.iter().filter_map(|item| item.error_kind.clone()));
    let answer_digests = count_by_string(
        outcomes
            .iter()
            .filter_map(|item| item.answer_digest.clone()),
    );
    let event_digests =
        count_by_string(outcomes.iter().filter_map(|item| item.event_digest.clone()));

    let sample_failures = outcomes
        .iter()
        .filter(|item| !item.ok)
        .take(MAX_FAILURE_SAMPLES)
        .cloned()
        .collect::<Vec<_>>();

    let total = outcomes.len();
    let success_rate = ratio(success, total);
    let error_rate = ratio(failed, total);
    let final_event_missing_rate = ratio(final_missing, total);

    SimulationReport {
        generated_at,
        args: SimArgsSnapshot::from_args(args),
        summary: SummaryStats {
            requests: total,
            concurrency: args.concurrency,
            success,
            failed,
            success_rate,
            error_rate,
            final_event_missing: final_missing,
            final_event_missing_rate,
            monotonic_violation,
            elapsed_ms,
            throughput_rps: if elapsed_ms > 0.0 {
                total as f64 / (elapsed_ms / 1000.0)
            } else {
                0.0
            },
        },
        latency_ms: DistributionStats::from_values(&latencies),
        first_event_latency_ms: if first_event_latencies.is_empty() {
            None
        } else {
            Some(DistributionStats::from_values(&first_event_latencies))
        },
        event_count: DistributionStats::from_values(&event_counts),
        stop_reasons,
        error_kinds,
        answer_digests,
        event_digests,
        top_answer_digest: top_digest(&outcomes, true),
        top_event_digest: top_digest(&outcomes, false),
        sample_failures,
        drift: None,
    }
}
fn compare_with_baseline(
    baseline: &BaselineSignature,
    report: &SimulationReport,
    args: &SimArgs,
) -> DriftResult {
    let current = BaselineSignature::from_report(report);
    let mut violations = Vec::new();

    if baseline.stream != current.stream {
        violations.push(format!(
            "stream mode changed: baseline={}, current={}",
            baseline.stream, current.stream
        ));
    }

    let error_rate_increase = (current.error_rate - baseline.error_rate).max(0.0);
    if error_rate_increase > args.max_error_rate_drift {
        violations.push(format!(
            "error_rate increase {:.4} > threshold {:.4}",
            error_rate_increase, args.max_error_rate_drift
        ));
    }

    let p95_latency_increase_ms = (current.p95_latency_ms - baseline.p95_latency_ms).max(0.0);
    if p95_latency_increase_ms > args.max_p95_latency_drift_ms {
        violations.push(format!(
            "p95 latency increase {:.2}ms > threshold {:.2}ms",
            p95_latency_increase_ms, args.max_p95_latency_drift_ms
        ));
    }

    let final_missing_rate_increase =
        (current.final_event_missing_rate - baseline.final_event_missing_rate).max(0.0);
    if final_missing_rate_increase > args.max_final_miss_rate_drift {
        violations.push(format!(
            "final_event_missing_rate increase {:.4} > threshold {:.4}",
            final_missing_rate_increase, args.max_final_miss_rate_drift
        ));
    }

    let top_answer_changed = baseline.top_answer_digest != current.top_answer_digest
        && baseline.top_answer_digest.is_some()
        && current.top_answer_digest.is_some();
    if top_answer_changed {
        violations.push(format!(
            "top answer digest changed: baseline={:?}, current={:?}",
            baseline.top_answer_digest, current.top_answer_digest
        ));
    }

    let top_event_changed = baseline.top_event_digest != current.top_event_digest
        && baseline.top_event_digest.is_some()
        && current.top_event_digest.is_some();
    if top_event_changed {
        violations.push(format!(
            "top event digest changed: baseline={:?}, current={:?}",
            baseline.top_event_digest, current.top_event_digest
        ));
    }

    DriftResult {
        passed: violations.is_empty(),
        violations,
        deltas: DriftDeltas {
            error_rate_increase,
            p95_latency_increase_ms,
            final_missing_rate_increase,
            top_answer_changed,
            top_event_changed,
        },
        baseline: baseline.clone(),
        current,
    }
}

fn print_summary(report: &SimulationReport) {
    println!(
        "[backend_sim] requests={} success={} failed={} success_rate={:.2}% p95={:.2}ms p99={:.2}ms rps={:.2}",
        report.summary.requests,
        report.summary.success,
        report.summary.failed,
        report.summary.success_rate * 100.0,
        report.latency_ms.p95,
        report.latency_ms.p99,
        report.summary.throughput_rps,
    );

    if let Some(first_event) = &report.first_event_latency_ms {
        println!(
            "[backend_sim] first_event p50={:.2}ms p95={:.2}ms p99={:.2}ms",
            first_event.p50, first_event.p95, first_event.p99
        );
    }

    println!(
        "[backend_sim] final_missing={} monotonic_violation={}",
        report.summary.final_event_missing, report.summary.monotonic_violation
    );
}

fn print_drift(drift: &DriftResult) {
    println!(
        "[backend_sim] drift_check={} error_delta={:.4} p95_delta_ms={:.2} final_missing_delta={:.4}",
        if drift.passed { "pass" } else { "fail" },
        drift.deltas.error_rate_increase,
        drift.deltas.p95_latency_increase_ms,
        drift.deltas.final_missing_rate_increase,
    );
    if !drift.violations.is_empty() {
        for item in &drift.violations {
            println!("[backend_sim] drift_violation: {item}");
        }
    }
}

fn sim_config_overrides() -> Value {
    json!({
        "llm": {
            "default": MOCK_MODEL_NAME,
            "models": {
                "__sim_mock__": {
                    "mock_if_unconfigured": true
                }
            }
        }
    })
}

fn count_by_string<I>(iter: I) -> HashMap<String, u64>
where
    I: IntoIterator<Item = String>,
{
    let mut map = HashMap::new();
    for item in iter {
        *map.entry(item).or_insert(0) += 1;
    }
    map
}

fn top_digest(outcomes: &[RequestOutcome], answer: bool) -> Option<DigestCount> {
    let mut map: HashMap<String, u64> = HashMap::new();
    for item in outcomes {
        let digest = if answer {
            item.answer_digest.as_ref()
        } else {
            item.event_digest.as_ref()
        };
        if let Some(value) = digest {
            *map.entry(value.clone()).or_insert(0) += 1;
        }
    }

    map.into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(digest, count)| DigestCount { digest, count })
}

fn digest_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    short_hash_hex(&hasher.finalize())
}

fn short_hash_hex(bytes: &[u8]) -> String {
    let mut encoded = hex::encode(bytes);
    encoded.truncate(16);
    encoded
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn classify_error(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("user_busy") {
        "USER_BUSY".to_string()
    } else if lower.contains("timeout") {
        "TIMEOUT".to_string()
    } else if lower.contains("cancel") {
        "CANCELLED".to_string()
    } else if lower.contains("llm") && lower.contains("unavailable") {
        "LLM_UNAVAILABLE".to_string()
    } else {
        "ERROR".to_string()
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.to_string_lossy()))?;
    }
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    fs::write(path, text)
        .with_context(|| format!("failed to write file: {}", path.to_string_lossy()))?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read file: {}", path.to_string_lossy()))?;
    let parsed = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse json file: {}", path.to_string_lossy()))?;
    Ok(parsed)
}

#[derive(Debug, Clone)]
struct SimArgs {
    requests: usize,
    concurrency: usize,
    stream: bool,
    skip_tool_calls: bool,
    question: String,
    user_prefix: String,
    session_mode: SessionMode,
    fixed_session_id: Option<String>,
    report: Option<PathBuf>,
    baseline: Option<PathBuf>,
    write_baseline: bool,
    fail_on_drift: bool,
    max_error_rate_drift: f64,
    max_p95_latency_drift_ms: f64,
    max_final_miss_rate_drift: f64,
}

impl Default for SimArgs {
    fn default() -> Self {
        Self {
            requests: DEFAULT_REQUESTS,
            concurrency: DEFAULT_CONCURRENCY,
            stream: true,
            skip_tool_calls: true,
            question: "Please summarize this backend in one sentence.".to_string(),
            user_prefix: "backend_sim_user".to_string(),
            session_mode: SessionMode::Unique,
            fixed_session_id: None,
            report: Some(PathBuf::from(DEFAULT_REPORT_PATH)),
            baseline: None,
            write_baseline: false,
            fail_on_drift: false,
            max_error_rate_drift: 0.02,
            max_p95_latency_drift_ms: 200.0,
            max_final_miss_rate_drift: 0.01,
        }
    }
}
impl SimArgs {
    fn parse_from_env(raw_args: Vec<String>) -> Result<Self> {
        let mut args = Self::default();
        let mut index = 0usize;

        while index < raw_args.len() {
            let flag = &raw_args[index];
            match flag.as_str() {
                "-h" | "--help" => {
                    Self::print_help();
                    std::process::exit(0);
                }
                "--requests" => {
                    args.requests = parse_usize(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                "--concurrency" => {
                    args.concurrency = parse_usize(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                "--stream" => {
                    args.stream = parse_bool(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                "--skip-tool-calls" => {
                    args.skip_tool_calls =
                        parse_bool(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                "--question" => {
                    args.question = next_arg(&raw_args, &mut index, flag)?;
                }
                "--user-prefix" => {
                    args.user_prefix = next_arg(&raw_args, &mut index, flag)?;
                }
                "--session-mode" => {
                    let raw_mode = next_arg(&raw_args, &mut index, flag)?;
                    args.session_mode = SessionMode::parse(&raw_mode).ok_or_else(|| {
                        anyhow!(
                            "invalid session mode: {raw_mode}; expected one of unique/shared/fixed"
                        )
                    })?;
                }
                "--session-id" => {
                    args.fixed_session_id = Some(next_arg(&raw_args, &mut index, flag)?);
                }
                "--report" => {
                    args.report = Some(PathBuf::from(next_arg(&raw_args, &mut index, flag)?));
                }
                "--baseline" => {
                    args.baseline = Some(PathBuf::from(next_arg(&raw_args, &mut index, flag)?));
                }
                "--write-baseline" => {
                    args.write_baseline = true;
                }
                "--fail-on-drift" => {
                    args.fail_on_drift = true;
                }
                "--max-error-rate-drift" => {
                    args.max_error_rate_drift =
                        parse_non_negative_f64(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                "--max-p95-latency-drift-ms" => {
                    args.max_p95_latency_drift_ms =
                        parse_non_negative_f64(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                "--max-final-miss-rate-drift" => {
                    args.max_final_miss_rate_drift =
                        parse_non_negative_f64(&next_arg(&raw_args, &mut index, flag)?, flag)?;
                }
                other => {
                    return Err(anyhow!("unknown argument: {other}"));
                }
            }
            index += 1;
        }

        if args.requests == 0 {
            return Err(anyhow!("--requests must be greater than 0"));
        }
        if args.concurrency == 0 {
            return Err(anyhow!("--concurrency must be greater than 0"));
        }

        args.concurrency = args.concurrency.min(args.requests);

        if matches!(args.session_mode, SessionMode::Fixed) && args.fixed_session_id.is_none() {
            return Err(anyhow!(
                "--session-id is required when --session-mode is fixed"
            ));
        }

        if (args.write_baseline || args.fail_on_drift) && args.baseline.is_none() {
            args.baseline = Some(PathBuf::from(DEFAULT_BASELINE_PATH));
        }

        if args.fail_on_drift {
            let baseline = args
                .baseline
                .as_ref()
                .ok_or_else(|| anyhow!("--baseline is required for --fail-on-drift"))?;
            if !baseline.exists() {
                return Err(anyhow!(
                    "baseline file does not exist: {}",
                    baseline.to_string_lossy()
                ));
            }
        }

        Ok(args)
    }

    fn resolve_session_id(&self, request_index: usize, shared_session_id: Option<&str>) -> String {
        match self.session_mode {
            SessionMode::Unique => {
                format!("backend_sim_{request_index}_{}", Uuid::new_v4().simple())
            }
            SessionMode::Shared | SessionMode::Fixed => shared_session_id
                .unwrap_or("backend_sim_shared")
                .to_string(),
        }
    }

    fn shared_session_id(&self) -> Option<String> {
        match self.session_mode {
            SessionMode::Unique => None,
            SessionMode::Shared => Some(format!("backend_sim_shared_{}", Uuid::new_v4().simple())),
            SessionMode::Fixed => self.fixed_session_id.clone(),
        }
    }

    fn print_help() {
        println!(
            "backend_sim\n\nUsage:\n  cargo run --bin backend_sim -- [options]\n\nOptions:\n  --requests <n>                     total requests (default: {DEFAULT_REQUESTS})\n  --concurrency <n>                  max in-flight requests (default: {DEFAULT_CONCURRENCY})\n  --stream <true|false>              use orchestrator stream path (default: true)\n  --skip-tool-calls <true|false>     bypass tool execution (default: true)\n  --question <text>                  input question payload\n  --user-prefix <prefix>             user id prefix\n  --session-mode <unique|shared|fixed>\n  --session-id <id>                  required when session-mode=fixed\n  --report <path>                    report path (default: {DEFAULT_REPORT_PATH})\n  --baseline <path>                  baseline path for write/compare\n  --write-baseline                   write baseline from current run\n  --fail-on-drift                    non-zero exit when drift violates thresholds\n  --max-error-rate-drift <f64>       allowed increase of error_rate (default: 0.02)\n  --max-p95-latency-drift-ms <f64>   allowed increase of p95 latency in ms (default: 200)\n  --max-final-miss-rate-drift <f64>  allowed increase of final miss rate (default: 0.01)\n  -h, --help                         show this help\n"
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum SessionMode {
    Unique,
    Shared,
    Fixed,
}

impl SessionMode {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "unique" => Some(Self::Unique),
            "shared" => Some(Self::Shared),
            "fixed" => Some(Self::Fixed),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Unique => "unique",
            Self::Shared => "shared",
            Self::Fixed => "fixed",
        }
    }
}

fn next_arg(raw_args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    raw_args
        .get(*index)
        .cloned()
        .ok_or_else(|| anyhow!("missing value for {flag}"))
}

fn parse_usize(raw: &str, flag: &str) -> Result<usize> {
    raw.parse::<usize>()
        .with_context(|| format!("invalid usize for {flag}: {raw}"))
}

fn parse_non_negative_f64(raw: &str, flag: &str) -> Result<f64> {
    let value = raw
        .parse::<f64>()
        .with_context(|| format!("invalid float for {flag}: {raw}"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(anyhow!("{flag} must be a finite non-negative number"));
    }
    Ok(value)
}

fn parse_bool(raw: &str, flag: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "on" => Ok(true),
        "false" | "0" | "no" | "n" | "off" => Ok(false),
        _ => Err(anyhow!("invalid boolean for {flag}: {raw}")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SimulationReport {
    generated_at: DateTime<Utc>,
    args: SimArgsSnapshot,
    summary: SummaryStats,
    latency_ms: DistributionStats,
    first_event_latency_ms: Option<DistributionStats>,
    event_count: DistributionStats,
    stop_reasons: HashMap<String, u64>,
    error_kinds: HashMap<String, u64>,
    answer_digests: HashMap<String, u64>,
    event_digests: HashMap<String, u64>,
    top_answer_digest: Option<DigestCount>,
    top_event_digest: Option<DigestCount>,
    sample_failures: Vec<RequestOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    drift: Option<DriftResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SimArgsSnapshot {
    requests: usize,
    concurrency: usize,
    stream: bool,
    skip_tool_calls: bool,
    question: String,
    user_prefix: String,
    session_mode: String,
    session_id: Option<String>,
    report: Option<String>,
    baseline: Option<String>,
    write_baseline: bool,
    fail_on_drift: bool,
    max_error_rate_drift: f64,
    max_p95_latency_drift_ms: f64,
    max_final_miss_rate_drift: f64,
}

impl SimArgsSnapshot {
    fn from_args(args: &SimArgs) -> Self {
        Self {
            requests: args.requests,
            concurrency: args.concurrency,
            stream: args.stream,
            skip_tool_calls: args.skip_tool_calls,
            question: args.question.clone(),
            user_prefix: args.user_prefix.clone(),
            session_mode: args.session_mode.as_str().to_string(),
            session_id: args.fixed_session_id.clone(),
            report: args
                .report
                .as_ref()
                .map(|path| path_to_string(path.as_path())),
            baseline: args
                .baseline
                .as_ref()
                .map(|path| path_to_string(path.as_path())),
            write_baseline: args.write_baseline,
            fail_on_drift: args.fail_on_drift,
            max_error_rate_drift: args.max_error_rate_drift,
            max_p95_latency_drift_ms: args.max_p95_latency_drift_ms,
            max_final_miss_rate_drift: args.max_final_miss_rate_drift,
        }
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SummaryStats {
    requests: usize,
    concurrency: usize,
    success: usize,
    failed: usize,
    success_rate: f64,
    error_rate: f64,
    final_event_missing: usize,
    final_event_missing_rate: f64,
    monotonic_violation: usize,
    elapsed_ms: f64,
    throughput_rps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DistributionStats {
    count: usize,
    min: f64,
    mean: f64,
    max: f64,
    p50: f64,
    p90: f64,
    p95: f64,
    p99: f64,
}

impl DistributionStats {
    fn from_values(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                count: 0,
                min: 0.0,
                mean: 0.0,
                max: 0.0,
                p50: 0.0,
                p90: 0.0,
                p95: 0.0,
                p99: 0.0,
            };
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|left, right| left.total_cmp(right));
        let total = sorted.iter().sum::<f64>();

        Self {
            count: sorted.len(),
            min: *sorted.first().unwrap_or(&0.0),
            mean: total / sorted.len() as f64,
            max: *sorted.last().unwrap_or(&0.0),
            p50: percentile(&sorted, 0.50),
            p90: percentile(&sorted, 0.90),
            p95: percentile(&sorted, 0.95),
            p99: percentile(&sorted, 0.99),
        }
    }
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let clamped = percentile.clamp(0.0, 1.0);
    let span = (sorted.len() - 1) as f64;
    let rank = clamped * span;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;

    if lower == upper {
        sorted[lower]
    } else {
        let weight = rank - lower as f64;
        sorted[lower] * (1.0 - weight) + sorted[upper] * weight
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DigestCount {
    digest: String,
    count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RequestOutcome {
    request_index: usize,
    user_id: String,
    session_id: String,
    ok: bool,
    latency_ms: f64,
    first_event_latency_ms: Option<f64>,
    event_count: usize,
    final_event_seen: bool,
    event_id_monotonic: bool,
    stop_reason: Option<String>,
    answer_digest: Option<String>,
    event_digest: Option<String>,
    error_kind: Option<String>,
    error_message: Option<String>,
}

impl RequestOutcome {
    fn panic_failure(request_index: usize, message: String) -> Self {
        Self {
            request_index,
            user_id: "panic".to_string(),
            session_id: "panic".to_string(),
            ok: false,
            latency_ms: 0.0,
            first_event_latency_ms: None,
            event_count: 0,
            final_event_seen: false,
            event_id_monotonic: true,
            stop_reason: None,
            answer_digest: None,
            event_digest: None,
            error_kind: Some("PANIC".to_string()),
            error_message: Some(message),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BaselineFile {
    generated_at: DateTime<Utc>,
    signature: BaselineSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BaselineSignature {
    stream: bool,
    success_rate: f64,
    error_rate: f64,
    final_event_missing_rate: f64,
    p95_latency_ms: f64,
    top_answer_digest: Option<String>,
    top_event_digest: Option<String>,
}

impl BaselineSignature {
    fn from_report(report: &SimulationReport) -> Self {
        Self {
            stream: report.args.stream,
            success_rate: report.summary.success_rate,
            error_rate: report.summary.error_rate,
            final_event_missing_rate: report.summary.final_event_missing_rate,
            p95_latency_ms: report.latency_ms.p95,
            top_answer_digest: report
                .top_answer_digest
                .as_ref()
                .map(|item| item.digest.clone()),
            top_event_digest: report
                .top_event_digest
                .as_ref()
                .map(|item| item.digest.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DriftResult {
    passed: bool,
    violations: Vec<String>,
    deltas: DriftDeltas,
    baseline: BaselineSignature,
    current: BaselineSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DriftDeltas {
    error_rate_increase: f64,
    p95_latency_increase_ms: f64,
    final_missing_rate_increase: f64,
    top_answer_changed: bool,
    top_event_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_interpolates_expected_value() {
        let sorted = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(percentile(&sorted, 0.0), 10.0);
        assert_eq!(percentile(&sorted, 1.0), 50.0);
        assert_eq!(percentile(&sorted, 0.5), 30.0);
        assert_eq!(percentile(&sorted, 0.75), 40.0);
    }

    #[test]
    fn drift_compare_flags_latency_regression() {
        let baseline = BaselineSignature {
            stream: true,
            success_rate: 1.0,
            error_rate: 0.0,
            final_event_missing_rate: 0.0,
            p95_latency_ms: 100.0,
            top_answer_digest: Some("aaaa".to_string()),
            top_event_digest: Some("bbbb".to_string()),
        };

        let report = SimulationReport {
            generated_at: Utc::now(),
            args: SimArgsSnapshot {
                requests: 10,
                concurrency: 2,
                stream: true,
                skip_tool_calls: true,
                question: "q".to_string(),
                user_prefix: "u".to_string(),
                session_mode: "unique".to_string(),
                session_id: None,
                report: None,
                baseline: None,
                write_baseline: false,
                fail_on_drift: false,
                max_error_rate_drift: 0.02,
                max_p95_latency_drift_ms: 200.0,
                max_final_miss_rate_drift: 0.01,
            },
            summary: SummaryStats {
                requests: 10,
                concurrency: 2,
                success: 10,
                failed: 0,
                success_rate: 1.0,
                error_rate: 0.0,
                final_event_missing: 0,
                final_event_missing_rate: 0.0,
                monotonic_violation: 0,
                elapsed_ms: 1000.0,
                throughput_rps: 10.0,
            },
            latency_ms: DistributionStats {
                count: 10,
                min: 50.0,
                mean: 120.0,
                max: 400.0,
                p50: 110.0,
                p90: 250.0,
                p95: 350.0,
                p99: 390.0,
            },
            first_event_latency_ms: None,
            event_count: DistributionStats::from_values(&[0.0]),
            stop_reasons: HashMap::new(),
            error_kinds: HashMap::new(),
            answer_digests: HashMap::new(),
            event_digests: HashMap::new(),
            top_answer_digest: Some(DigestCount {
                digest: "aaaa".to_string(),
                count: 10,
            }),
            top_event_digest: Some(DigestCount {
                digest: "bbbb".to_string(),
                count: 10,
            }),
            sample_failures: Vec::new(),
            drift: None,
        };

        let args = SimArgs {
            max_p95_latency_drift_ms: 50.0,
            fail_on_drift: true,
            ..SimArgs::default()
        };

        let drift = compare_with_baseline(&baseline, &report, &args);
        assert!(!drift.passed);
        assert!(drift.deltas.p95_latency_increase_ms > 200.0);
        assert!(!drift.violations.is_empty());
    }
}
