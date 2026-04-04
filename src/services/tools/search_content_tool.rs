use super::command_options::parse_dry_run;
use super::{
    collect_read_roots, resolve_tool_path, roots_allow_any_path,
    tool_error::build_failed_tool_result, tool_error::ToolErrorMeta, ToolContext, MAX_READ_BYTES,
    MAX_SEARCH_MATCHES,
};
use crate::core::{command_utils::is_not_found_error, tool_fs_filter};
use crate::i18n;
use anyhow::{anyhow, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

const MAX_CONTEXT_LINES: usize = 20;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_MAX_CANDIDATES: usize = 4000;
const MAX_MAX_CANDIDATES: usize = 20_000;
const MAX_MATCHES_CAP: usize = 2000;
const MIN_OUTPUT_BUDGET_BYTES: usize = 2 * 1024;
const MAX_OUTPUT_BUDGET_BYTES: usize = 4 * 1024 * 1024;
const BINARY_SAMPLE_BYTES: usize = 4096;
const CONTROL_BYTE_RATIO_THRESHOLD: f64 = 0.12;
const RG_BINARY_ENV: &str = "WUNDER_RG_BIN";
const DESKTOP_APP_DIR_ENV: &str = "WUNDER_DESKTOP_APP_DIR";
const RG_RELATIVE_DIRS: &[&str] = &[
    "opt/rg",
    "opt/rg/bin",
    "opt/ripgrep",
    "opt/ripgrep/bin",
    "resources/opt/rg",
    "resources/opt/rg/bin",
    "resources/opt/ripgrep",
    "resources/opt/ripgrep/bin",
    "resources/tools",
    "tools",
    "bin",
];
const DEFAULT_EXCLUDE_GLOBS: &[&str] = &[
    "**/.git/**",
    "**/target/**",
    "**/node_modules/**",
    "**/.next/**",
    "**/.nuxt/**",
    "**/.turbo/**",
    "**/.cache/**",
];

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum QueryMode {
    Literal,
    Regex,
}

impl QueryMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Regex => "regex",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum QuerySource {
    Query,
    Pattern,
}

impl QuerySource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Pattern => "pattern",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum SearchEngine {
    Auto,
    Rust,
    Rg,
}

impl SearchEngine {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Rust => "rust",
            Self::Rg => "rg",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum SearchStrategy {
    LiteralExact,
    LiteralDisjunction,
    Regex,
    LiteralTermsFallback,
}

impl SearchStrategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::LiteralExact => "literal_exact",
            Self::LiteralDisjunction => "literal_disjunction",
            Self::Regex => "regex",
            Self::LiteralTermsFallback => "literal_terms_fallback",
        }
    }
}

#[derive(Debug, Clone)]
struct SearchParams {
    query: String,
    query_source: QuerySource,
    path: String,
    file_pattern_items: Vec<String>,
    query_mode: QueryMode,
    query_mode_inferred: bool,
    case_sensitive: bool,
    context_before: usize,
    context_after: usize,
    max_depth: usize,
    max_files: usize,
    max_matches: usize,
    max_candidates: usize,
    timeout_ms: u64,
    engine: SearchEngine,
    output_budget_bytes: Option<usize>,
}

#[derive(Debug, Copy, Clone, Default)]
struct SearchBudget {
    time_budget_ms: Option<u64>,
    output_budget_bytes: Option<usize>,
    max_files: Option<usize>,
    max_matches: Option<usize>,
    max_candidates: Option<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
struct HighlightSegment {
    text: String,
    matched: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
struct ContextLine {
    line: usize,
    content: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
struct SearchHit {
    path: String,
    line: usize,
    content: String,
    segments: Vec<HighlightSegment>,
    matched_terms: Vec<String>,
    before: Vec<ContextLine>,
    after: Vec<ContextLine>,
}

#[derive(Debug, Clone, Serialize)]
struct SearchSummary {
    query_source: String,
    query_mode: String,
    query_mode_inferred: bool,
    strategy: String,
    fallback_applied: bool,
    returned_match_count: usize,
    matched_file_count: usize,
    top_files: Vec<String>,
    matched_terms: Vec<String>,
    focus_points: Vec<String>,
    next_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SearchAttemptTrace {
    strategy: String,
    query_mode: String,
    query_used: String,
    returned_match_count: usize,
    matched_file_count: usize,
    scanned_files: usize,
}

#[derive(Debug, Clone, Serialize)]
struct SearchExecutionMeta {
    requested_engine: String,
    resolved_engine: String,
    rg_program: Option<String>,
    fallback: bool,
    fallback_reason: Option<String>,
    elapsed_ms: u128,
    timeout_hit: bool,
    file_limit_hit: bool,
    match_limit_hit: bool,
    candidate_limit_hit: bool,
    output_budget_hit: bool,
    scanned_files: usize,
    query_source: String,
    query_mode_inferred: bool,
    strategy: String,
    fallback_applied: bool,
    attempts_tried: Vec<SearchAttemptTrace>,
}

#[derive(Debug, Clone)]
struct SearchComputation {
    hits: Vec<SearchHit>,
    scanned_files: usize,
    timeout_hit: bool,
    file_limit_hit: bool,
    match_limit_hit: bool,
}

#[derive(Debug, Clone)]
struct RgCandidateResult {
    paths: Vec<PathBuf>,
    rg_program: String,
    timeout_hit: bool,
    candidate_limit_hit: bool,
}

#[derive(Debug, Clone)]
struct RgLaunchCandidate {
    program: OsString,
    display: String,
}

#[derive(Debug, Clone)]
struct SearchAttempt {
    strategy: SearchStrategy,
    query: String,
    query_mode: QueryMode,
    matcher: Arc<Regex>,
    match_terms: Vec<String>,
    preferred_phrase: Option<String>,
}

#[derive(Debug, Clone)]
struct SearchAttemptResult {
    hits: Vec<SearchHit>,
    scanned_files: usize,
    timeout_hit: bool,
    file_limit_hit: bool,
    match_limit_hit: bool,
    resolved_engine: SearchEngine,
    rg_program: Option<String>,
    fallback_reason: Option<String>,
    candidate_limit_hit: bool,
}

pub(super) async fn search_content(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let dry_run = parse_dry_run(args);
    let params = match parse_search_params(args) {
        Ok(value) => value,
        Err(error) => {
            return Ok(build_failed_tool_result(
                error.to_string(),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_SEARCH_INVALID_ARGS",
                    Some("Please check query/path/engine/budget argument formats.".to_string()),
                    false,
                    None,
                ),
                false,
            ));
        }
    };

    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    let unrestricted_paths = roots_allow_any_path(&extra_roots);
    let root = {
        let path = params.path.clone();
        tokio::task::spawn_blocking(move || {
            resolve_tool_path(workspace.as_ref(), &user_id, &path, &extra_roots)
        })
        .await
        .map_err(|err| anyhow!(err.to_string()))??
    };
    if !root.exists() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.search.path_not_found"),
            json!({ "path": params.path }),
            ToolErrorMeta::new(
                "TOOL_SEARCH_PATH_NOT_FOUND",
                Some("Ensure path exists and stays within allowed roots.".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    let resolved_path = root.to_string_lossy().to_string();
    let scope = build_search_scope(&resolved_path);
    let scope_note = build_search_scope_note(&resolved_path);

    let file_filter = match build_file_filter(&params.file_pattern_items) {
        Ok(value) => value,
        Err(err) => {
            return Ok(build_failed_tool_result(
                err.to_string(),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_SEARCH_INVALID_FILE_PATTERN",
                    Some("Please check glob syntax in file_pattern or budget options.".to_string()),
                    false,
                    None,
                ),
                false,
            ));
        }
    };

    let attempts = match build_search_attempts(&params) {
        Ok(value) => value,
        Err(err) => {
            return Ok(build_failed_tool_result(
                err.to_string(),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_SEARCH_INVALID_QUERY",
                    Some(
                        "Please check query, pattern, or regex validity when query_mode=regex."
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };

    if dry_run {
        return Ok(json!({
            "ok": true,
            "data": {
                "dry_run": true,
                "path": root.to_string_lossy().to_string(),
                "resolved_path": resolved_path,
                "scope": scope,
                "scope_note": scope_note,
                "query_source": params.query_source.as_str(),
                "query_mode": params.query_mode.as_str(),
                "query_mode_inferred": params.query_mode_inferred,
                "case_sensitive": params.case_sensitive,
                "engine": params.engine.as_str(),
                "file_pattern_items": params.file_pattern_items,
                "context_before": params.context_before,
                "context_after": params.context_after,
                "max_depth": params.max_depth,
                "max_files": params.max_files,
                "max_matches": params.max_matches,
                "max_candidates": params.max_candidates,
                "timeout_ms": params.timeout_ms,
                "output_budget_bytes": params.output_budget_bytes,
                "attempts": attempts
                    .iter()
                    .map(|attempt| json!({
                        "strategy": attempt.strategy.as_str(),
                        "query_mode": attempt.query_mode.as_str(),
                        "query_used": attempt.query.clone(),
                    }))
                    .collect::<Vec<_>>(),
            },
            "error": "",
        }));
    }

    let started_at = Instant::now();
    let deadline = started_at + Duration::from_millis(params.timeout_ms);
    let rg_launch_candidates =
        resolve_rg_launch_candidates(context.config.tools.search.rg_path.as_deref());
    let mut selected_attempt: Option<&SearchAttempt> = None;
    let mut selected_result: Option<SearchAttemptResult> = None;
    let mut last_result: Option<SearchAttemptResult> = None;
    let mut attempt_traces = Vec::new();
    let mut any_timeout_hit = false;

    for attempt in &attempts {
        let result = match execute_search_attempt(
            &root,
            &params,
            attempt,
            file_filter.as_ref(),
            unrestricted_paths,
            deadline,
            &rg_launch_candidates,
        )
        .await
        {
            Ok(value) => value,
            Err(err) => {
                if params.engine == SearchEngine::Rg {
                    return Ok(build_failed_tool_result(
                        err.to_string(),
                        json!({
                            "engine": "rg",
                        }),
                        ToolErrorMeta::new(
                            "TOOL_SEARCH_RG_FAILED",
                            Some(
                                "rg fast path failed; try engine=rust or narrow the query scope."
                                    .to_string(),
                            ),
                            true,
                            Some(200),
                        ),
                        false,
                    ));
                }
                attempt_traces.push(SearchAttemptTrace {
                    strategy: attempt.strategy.as_str().to_string(),
                    query_mode: attempt.query_mode.as_str().to_string(),
                    query_used: attempt.query.clone(),
                    returned_match_count: 0,
                    matched_file_count: 0,
                    scanned_files: 0,
                });
                continue;
            }
        };
        any_timeout_hit |= result.timeout_hit;
        attempt_traces.push(SearchAttemptTrace {
            strategy: attempt.strategy.as_str().to_string(),
            query_mode: attempt.query_mode.as_str().to_string(),
            query_used: attempt.query.clone(),
            returned_match_count: result.hits.len(),
            matched_file_count: collect_matched_files(&result.hits).len(),
            scanned_files: result.scanned_files,
        });
        if !result.hits.is_empty() {
            selected_attempt = Some(attempt);
            selected_result = Some(result);
            break;
        }
        last_result = Some(result);
    }

    let selected_attempt = selected_attempt.unwrap_or_else(|| &attempts[attempts.len() - 1]);
    let selected_result = selected_result
        .or(last_result)
        .unwrap_or(SearchAttemptResult {
            hits: Vec::new(),
            scanned_files: 0,
            timeout_hit: any_timeout_hit,
            file_limit_hit: false,
            match_limit_hit: false,
            resolved_engine: SearchEngine::Rust,
            rg_program: None,
            fallback_reason: None,
            candidate_limit_hit: false,
        });
    let SearchAttemptResult {
        hits: raw_hits,
        scanned_files,
        timeout_hit: selected_timeout_hit,
        file_limit_hit,
        match_limit_hit,
        resolved_engine,
        rg_program,
        fallback_reason,
        candidate_limit_hit,
    } = selected_result;

    let mut hits = rank_search_hits(raw_hits, selected_attempt, params.case_sensitive);
    if hits.len() > params.max_matches {
        hits.truncate(params.max_matches);
    }
    let (hits, output_budget_hit) = limit_hits_by_output_budget(hits, params.output_budget_bytes);
    let matched_files = collect_matched_files(&hits);
    let matched_file_count = matched_files.len();
    let returned_match_count = hits.len();
    let matches = hits
        .iter()
        .map(|item| format!("{}:{}:{}", item.path, item.line, item.content.trim()))
        .collect::<Vec<_>>();

    let elapsed_ms = started_at.elapsed().as_millis();
    let timeout_hit = any_timeout_hit || selected_timeout_hit;
    let fallback = params.engine == SearchEngine::Auto && resolved_engine == SearchEngine::Rust;
    let summary = build_search_summary(
        &params,
        selected_attempt,
        &attempt_traces,
        &matched_files,
        &hits,
        scanned_files,
        output_budget_hit,
    );
    let meta = SearchExecutionMeta {
        requested_engine: params.engine.as_str().to_string(),
        resolved_engine: resolved_engine.as_str().to_string(),
        rg_program,
        fallback,
        fallback_reason,
        elapsed_ms,
        timeout_hit,
        file_limit_hit,
        match_limit_hit,
        candidate_limit_hit,
        output_budget_hit,
        scanned_files,
        query_source: params.query_source.as_str().to_string(),
        query_mode_inferred: params.query_mode_inferred,
        strategy: selected_attempt.strategy.as_str().to_string(),
        fallback_applied: selected_attempt.strategy == SearchStrategy::LiteralTermsFallback,
        attempts_tried: attempt_traces.clone(),
    };

    Ok(json!({
        "query": params.query,
        "query_source": params.query_source.as_str(),
        "query_mode_inferred": params.query_mode_inferred,
        "query_used": selected_attempt.query.clone(),
        "path": params.path,
        "resolved_path": resolved_path,
        "scope": scope,
        "scope_note": scope_note,
        "summary": summary,
        "matches": matches,
        "hits": hits,
        "matched_files": matched_files,
        "matched_file_count": matched_file_count,
        "returned_match_count": returned_match_count,
        "file_pattern_items": params.file_pattern_items,
        "query_mode": selected_attempt.query_mode.as_str(),
        "strategy": selected_attempt.strategy.as_str(),
        "case_sensitive": params.case_sensitive,
        "context_before": params.context_before,
        "context_after": params.context_after,
        "scanned_files": scanned_files,
        "file_limit_hit": file_limit_hit,
        "match_limit_hit": match_limit_hit,
        "timeout_hit": timeout_hit,
        "engine": resolved_engine.as_str(),
        "budget": {
            "time_budget_ms": params.timeout_ms,
            "output_budget_bytes": params.output_budget_bytes,
            "max_files": if params.max_files > 0 { Some(params.max_files) } else { None },
            "max_matches": params.max_matches,
            "max_candidates": params.max_candidates,
        },
        "meta": {
            "search": meta,
        },
    }))
}

fn parse_search_params(args: &Value) -> Result<SearchParams> {
    let (query, query_source) = parse_search_query(args);
    if query.is_empty() {
        return Err(anyhow!(i18n::t("tool.search.empty")));
    }

    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .trim()
        .to_string();
    let file_pattern_items = collect_file_pattern_items(args);
    let (query_mode, query_mode_inferred) = parse_query_mode(args, query_source);
    let case_sensitive = parse_case_sensitive(args);
    let (context_before, context_after) = parse_context_windows(args);
    let budget = parse_search_budget(args);
    let max_depth = parse_optional_usize(args.get("max_depth")).unwrap_or(0);
    let mut max_files = parse_optional_usize(args.get("max_files")).unwrap_or(0);
    let mut max_matches = parse_optional_usize(args.get("max_matches"))
        .or_else(|| parse_optional_usize(args.get("max_count")))
        .or_else(|| parse_optional_usize(args.get("head_limit")))
        .unwrap_or(MAX_SEARCH_MATCHES)
        .clamp(1, MAX_MATCHES_CAP);
    let mut max_candidates = parse_optional_usize(args.get("max_candidates"))
        .unwrap_or(DEFAULT_MAX_CANDIDATES)
        .clamp(1, MAX_MAX_CANDIDATES);
    let mut timeout_ms = parse_optional_u64(args.get("timeout_ms"))
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(1, MAX_TIMEOUT_MS);
    if let Some(limit) = budget.max_files {
        max_files = if max_files == 0 {
            limit
        } else {
            max_files.min(limit)
        };
    }
    if let Some(limit) = budget.max_matches {
        max_matches = max_matches.min(limit.clamp(1, MAX_MATCHES_CAP));
    }
    if let Some(limit) = budget.max_candidates {
        max_candidates = max_candidates.min(limit.clamp(1, MAX_MAX_CANDIDATES));
    }
    if let Some(limit) = budget.time_budget_ms {
        timeout_ms = timeout_ms.min(limit.clamp(1, MAX_TIMEOUT_MS));
    }
    let output_budget_bytes = budget
        .output_budget_bytes
        .map(|value| value.clamp(MIN_OUTPUT_BUDGET_BYTES, MAX_OUTPUT_BUDGET_BYTES));
    let engine = parse_search_engine(args);

    Ok(SearchParams {
        query,
        query_source,
        path,
        file_pattern_items,
        query_mode,
        query_mode_inferred,
        case_sensitive,
        context_before,
        context_after,
        max_depth,
        max_files,
        max_matches,
        max_candidates,
        timeout_ms,
        engine,
        output_budget_bytes,
    })
}

fn parse_search_query(args: &Value) -> (String, QuerySource) {
    if let Some(query) = args
        .get("query")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return (query.trim().to_string(), QuerySource::Query);
    }
    let pattern = args
        .get("pattern")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string();
    (pattern, QuerySource::Pattern)
}

fn parse_search_budget(args: &Value) -> SearchBudget {
    let budget_obj = args.get("budget").and_then(Value::as_object);
    SearchBudget {
        time_budget_ms: budget_obj
            .and_then(|obj| obj.get("time_budget_ms"))
            .and_then(parse_optional_u64_value)
            .or_else(|| {
                args.get("time_budget_ms")
                    .and_then(parse_optional_u64_value)
            }),
        output_budget_bytes: budget_obj
            .and_then(|obj| obj.get("output_budget_bytes"))
            .and_then(parse_optional_usize_value)
            .or_else(|| {
                args.get("output_budget_bytes")
                    .and_then(parse_optional_usize_value)
            }),
        max_files: budget_obj
            .and_then(|obj| obj.get("max_files"))
            .and_then(parse_optional_usize_value),
        max_matches: budget_obj
            .and_then(|obj| obj.get("max_matches"))
            .and_then(parse_optional_usize_value),
        max_candidates: budget_obj
            .and_then(|obj| obj.get("max_candidates"))
            .and_then(parse_optional_usize_value),
    }
}

fn parse_optional_usize(value: Option<&Value>) -> Option<usize> {
    value.and_then(parse_optional_usize_value)
}

fn parse_optional_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(parse_optional_u64_value)
}

fn parse_optional_bool_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(num) => num.as_i64().map(|item| item != 0),
        Value::String(text) => {
            let cleaned = text.trim().to_ascii_lowercase();
            match cleaned.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn parse_optional_usize_value(value: &Value) -> Option<usize> {
    match value {
        Value::Number(num) => num.as_u64().map(|item| item as usize),
        Value::String(text) => text.trim().parse::<usize>().ok(),
        _ => None,
    }
}

fn parse_optional_u64_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(num) => num.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn parse_search_engine(args: &Value) -> SearchEngine {
    let raw = args
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "rust" => SearchEngine::Rust,
        "rg" => SearchEngine::Rg,
        _ => SearchEngine::Auto,
    }
}

fn parse_query_mode(args: &Value, query_source: QuerySource) -> (QueryMode, bool) {
    if let Some(raw) = args.get("query_mode").and_then(Value::as_str) {
        let cleaned = raw.trim().to_ascii_lowercase();
        if cleaned == "regex" || cleaned == "re" {
            return (QueryMode::Regex, false);
        }
        if cleaned == "literal" || cleaned == "fixed" || cleaned == "fixed_strings" {
            return (QueryMode::Literal, false);
        }
    }
    if args
        .get("fixed_strings")
        .and_then(parse_optional_bool_value)
        .unwrap_or(false)
        || args
            .get("-F")
            .and_then(parse_optional_bool_value)
            .unwrap_or(false)
        || args
            .get("literal")
            .and_then(parse_optional_bool_value)
            .unwrap_or(false)
    {
        return (QueryMode::Literal, false);
    }
    if args
        .get("regex")
        .and_then(parse_optional_bool_value)
        .unwrap_or(false)
    {
        return (QueryMode::Regex, false);
    }
    if query_source == QuerySource::Pattern {
        return (QueryMode::Regex, true);
    }
    (QueryMode::Literal, true)
}

fn parse_case_sensitive(args: &Value) -> bool {
    if let Some(case_sensitive) = args
        .get("case_sensitive")
        .and_then(parse_optional_bool_value)
    {
        return case_sensitive;
    }
    if let Some(ignore_case) = args
        .get("ignore_case")
        .and_then(parse_optional_bool_value)
        .or_else(|| {
            args.get("case_insensitive")
                .and_then(parse_optional_bool_value)
        })
        .or_else(|| args.get("-i").and_then(parse_optional_bool_value))
    {
        return !ignore_case;
    }
    false
}

fn parse_context_windows(args: &Value) -> (usize, usize) {
    let shared = args
        .get("context")
        .and_then(parse_optional_u64_value)
        .or_else(|| args.get("-C").and_then(parse_optional_u64_value));
    let before = args
        .get("context_before")
        .and_then(parse_optional_u64_value)
        .or_else(|| args.get("-B").and_then(parse_optional_u64_value))
        .or(shared);
    let after = args
        .get("context_after")
        .and_then(parse_optional_u64_value)
        .or_else(|| args.get("-A").and_then(parse_optional_u64_value))
        .or(shared);
    (
        normalize_context_window(before),
        normalize_context_window(after),
    )
}

fn normalize_context_window(raw: Option<u64>) -> usize {
    raw.unwrap_or(0).min(MAX_CONTEXT_LINES as u64) as usize
}

fn split_file_patterns(raw: &str) -> Vec<String> {
    raw.split([',', ';', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
}

fn collect_file_pattern_items(args: &Value) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for key in ["file_pattern", "glob"] {
        if let Some(raw) = args.get(key).and_then(Value::as_str) {
            for item in split_file_patterns(raw.trim()) {
                if seen.insert(item.clone()) {
                    output.push(item);
                }
            }
        }
    }
    for item in collect_type_patterns(args) {
        if seen.insert(item.clone()) {
            output.push(item);
        }
    }
    output
}

fn collect_type_patterns(args: &Value) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for key in ["type", "types"] {
        let Some(value) = args.get(key) else {
            continue;
        };
        for item in parse_type_items(value) {
            for glob in expand_type_globs(item.as_str()) {
                if seen.insert(glob.clone()) {
                    output.push(glob);
                }
            }
        }
    }
    output
}

fn parse_type_items(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => text
            .split([',', ';', '\n', ' '])
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Value::Array(items) => items.iter().flat_map(parse_type_items).collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

fn expand_type_globs(raw: &str) -> Vec<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let patterns: &[&str] = match normalized.as_str() {
        "rust" | "rs" => &["*.rs"],
        "typescript" | "ts" => &["*.ts", "*.mts", "*.cts"],
        "tsx" => &["*.tsx"],
        "javascript" | "js" => &["*.js", "*.mjs", "*.cjs"],
        "jsx" => &["*.jsx"],
        "vue" => &["*.vue"],
        "python" | "py" => &["*.py"],
        "go" => &["*.go"],
        "java" => &["*.java"],
        "kotlin" | "kt" => &["*.kt", "*.kts"],
        "swift" => &["*.swift"],
        "c" => &["*.c", "*.h"],
        "cpp" | "c++" | "cc" | "cxx" => &["*.cc", "*.cpp", "*.cxx", "*.hpp", "*.hh", "*.hxx"],
        "shell" | "sh" | "bash" => &["*.sh", "*.bash", "*.zsh"],
        "powershell" | "pwsh" | "ps1" => &["*.ps1", "*.psm1", "*.psd1"],
        "html" | "htm" => &["*.html", "*.htm"],
        "css" | "scss" | "sass" | "less" => &["*.css", "*.scss", "*.sass", "*.less"],
        "json" => &["*.json"],
        "yaml" | "yml" => &["*.yaml", "*.yml"],
        "toml" => &["*.toml"],
        "md" | "markdown" => &["*.md", "*.mdx"],
        "sql" => &["*.sql"],
        "xml" => &["*.xml"],
        "proto" | "protobuf" => &["*.proto"],
        "csv" => &["*.csv"],
        _ if normalized.is_empty() => &[],
        _ => &[],
    };
    if !patterns.is_empty() {
        return patterns.iter().map(|item| (*item).to_string()).collect();
    }
    vec![format!("*.{normalized}")]
}

fn literal_query_terms(query: &str) -> Vec<String> {
    let terms = query
        .split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        vec![query.to_string()]
    } else {
        terms
    }
}

fn trim_search_term(raw: &str) -> String {
    raw.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\''
                | '`'
                | ','
                | ';'
                | ':'
                | '|'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | '，'
                | '。'
                | '；'
                | '：'
                | '（'
                | '）'
                | '【'
                | '】'
        )
    })
    .trim()
    .to_string()
}

fn is_useful_fallback_term(term: &str) -> bool {
    let char_count = term.chars().count();
    if term
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return char_count >= 2;
    }
    char_count >= 2
}

fn literal_query_fallback_terms(query: &str) -> Vec<String> {
    if query.contains('|') {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for raw in query.split_whitespace() {
        let term = trim_search_term(raw);
        if term.is_empty() || !is_useful_fallback_term(&term) {
            continue;
        }
        let key = term.to_ascii_lowercase();
        if seen.insert(key) {
            output.push(term);
        }
    }
    if output.len() < 2 {
        return Vec::new();
    }
    output
}

fn build_query_matcher(query: &str, query_mode: QueryMode, case_sensitive: bool) -> Result<Regex> {
    let pattern = match query_mode {
        QueryMode::Literal => literal_query_terms(query)
            .iter()
            .map(|item| regex::escape(item))
            .collect::<Vec<_>>()
            .join("|"),
        QueryMode::Regex => query.to_string(),
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .unicode(true)
        .build()
        .map_err(|err| match query_mode {
            QueryMode::Regex => anyhow!(i18n::t_with_params(
                "tool.search.invalid_regex",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            )),
            QueryMode::Literal => anyhow!("build query matcher failed: {err}"),
        })
}

fn build_search_attempts(params: &SearchParams) -> Result<Vec<SearchAttempt>> {
    let mut attempts = Vec::new();
    let primary_strategy = match params.query_mode {
        QueryMode::Regex => SearchStrategy::Regex,
        QueryMode::Literal if params.query.contains('|') => SearchStrategy::LiteralDisjunction,
        QueryMode::Literal => SearchStrategy::LiteralExact,
    };
    attempts.push(build_search_attempt(
        primary_strategy,
        params.query.clone(),
        params.query_mode,
        literal_query_terms(&params.query),
        Some(params.query.clone()),
        params.case_sensitive,
    )?);
    if params.query_mode == QueryMode::Literal && params.query_source == QuerySource::Query {
        let fallback_terms = literal_query_fallback_terms(&params.query);
        if fallback_terms.len() >= 2 {
            attempts.push(build_search_attempt(
                SearchStrategy::LiteralTermsFallback,
                fallback_terms.join("|"),
                QueryMode::Literal,
                fallback_terms,
                Some(params.query.clone()),
                params.case_sensitive,
            )?);
        }
    }
    Ok(attempts)
}

fn build_search_attempt(
    strategy: SearchStrategy,
    query: String,
    query_mode: QueryMode,
    match_terms: Vec<String>,
    preferred_phrase: Option<String>,
    case_sensitive: bool,
) -> Result<SearchAttempt> {
    let matcher = Arc::new(build_query_matcher(&query, query_mode, case_sensitive)?);
    Ok(SearchAttempt {
        strategy,
        query,
        query_mode,
        matcher,
        match_terms,
        preferred_phrase,
    })
}

fn build_file_filter(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for item in patterns {
        let glob =
            Glob::new(item).map_err(|err| anyhow!("invalid file_pattern `{item}`: {err}"))?;
        builder.add(glob);
    }
    let set = builder
        .build()
        .map_err(|err| anyhow!("invalid file_pattern: {err}"))?;
    Ok(Some(set))
}

fn resolve_rg_launch_candidates(configured_rg_path: Option<&str>) -> Vec<RgLaunchCandidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let base_dirs = collect_rg_candidate_base_dirs();

    if let Ok(raw) = std::env::var(RG_BINARY_ENV) {
        push_rg_candidate_reference(&mut candidates, &mut seen, raw.trim(), &base_dirs);
    }
    if let Some(raw) = configured_rg_path {
        push_rg_candidate_reference(&mut candidates, &mut seen, raw.trim(), &base_dirs);
    }

    let binary_name = rg_binary_name();
    for base in &base_dirs {
        push_rg_candidate_path(&mut candidates, &mut seen, base.join(binary_name));
        for relative in RG_RELATIVE_DIRS {
            push_rg_candidate_path(
                &mut candidates,
                &mut seen,
                base.join(relative).join(binary_name),
            );
        }
    }

    push_rg_candidate_program(&mut candidates, &mut seen, "rg");
    candidates
}

fn collect_rg_candidate_base_dirs() -> Vec<PathBuf> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    if let Some(raw) = std::env::var_os(DESKTOP_APP_DIR_ENV) {
        push_base_dir_with_ancestors(&mut output, &mut seen, PathBuf::from(raw));
    }
    if let Some(raw) = std::env::var_os("APPDIR") {
        push_base_dir_with_ancestors(&mut output, &mut seen, PathBuf::from(raw));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_base_dir_with_ancestors(&mut output, &mut seen, parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        push_base_dir_with_ancestors(&mut output, &mut seen, cwd);
    }
    output
}

fn push_base_dir_with_ancestors(
    output: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    seed: PathBuf,
) {
    if !seed.exists() {
        return;
    }
    let mut current = Some(seed);
    for _ in 0..3 {
        let Some(path) = current else {
            break;
        };
        if path.is_dir() {
            let key = normalize_candidate_key(path.to_string_lossy().as_ref());
            if seen.insert(key) {
                output.push(path.clone());
            }
        }
        current = path.parent().map(Path::to_path_buf);
    }
}

fn push_rg_candidate_reference(
    candidates: &mut Vec<RgLaunchCandidate>,
    seen: &mut HashSet<String>,
    raw: &str,
    base_dirs: &[PathBuf],
) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let candidate = PathBuf::from(trimmed);
    if candidate.components().count() == 1 && !candidate.is_absolute() {
        push_rg_candidate_program(candidates, seen, trimmed);
        return;
    }
    if candidate.is_absolute() {
        push_rg_candidate_path(candidates, seen, candidate);
        return;
    }
    if let Ok(cwd) = std::env::current_dir() {
        push_rg_candidate_path(candidates, seen, cwd.join(&candidate));
    }
    for base in base_dirs {
        push_rg_candidate_path(candidates, seen, base.join(&candidate));
    }
}

fn push_rg_candidate_path(
    candidates: &mut Vec<RgLaunchCandidate>,
    seen: &mut HashSet<String>,
    path: PathBuf,
) {
    if !path.is_file() {
        return;
    }
    let display = path.to_string_lossy().to_string();
    let key = format!("path:{}", normalize_candidate_key(&display));
    if !seen.insert(key) {
        return;
    }
    candidates.push(RgLaunchCandidate {
        program: path.into_os_string(),
        display,
    });
}

fn push_rg_candidate_program(
    candidates: &mut Vec<RgLaunchCandidate>,
    seen: &mut HashSet<String>,
    program: &str,
) {
    let trimmed = program.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = format!("program:{}", normalize_candidate_key(trimmed));
    if !seen.insert(key) {
        return;
    }
    candidates.push(RgLaunchCandidate {
        program: OsString::from(trimmed),
        display: trimmed.to_string(),
    });
}

fn normalize_candidate_key(raw: &str) -> String {
    if cfg!(windows) {
        raw.to_ascii_lowercase()
    } else {
        raw.to_string()
    }
}

fn rg_binary_name() -> &'static str {
    if cfg!(windows) {
        "rg.exe"
    } else {
        "rg"
    }
}

fn build_rg_search_command(
    program: &OsString,
    cwd: &Path,
    target: &Path,
    query: &str,
    query_mode: QueryMode,
    params: &SearchParams,
    unrestricted_paths: bool,
) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(cwd)
        .arg("--files-with-matches")
        .arg("--no-messages")
        .arg("--color")
        .arg("never")
        .arg("--max-filesize")
        .arg(MAX_READ_BYTES.to_string());
    match query_mode {
        QueryMode::Literal => {
            command.arg("--fixed-strings");
            for item in literal_query_terms(query) {
                command.arg("--regexp").arg(item);
            }
        }
        QueryMode::Regex => {
            command.arg("--regexp").arg(query);
        }
    }
    if unrestricted_paths {
        command
            .arg("--hidden")
            .arg("--no-ignore")
            .arg("--no-ignore-global")
            .arg("--no-ignore-parent")
            .arg("--no-ignore-vcs");
    }
    if !params.case_sensitive {
        command.arg("--ignore-case");
    }
    if params.max_depth > 0 {
        command.arg("--max-depth").arg(params.max_depth.to_string());
    }
    if !unrestricted_paths {
        for glob in DEFAULT_EXCLUDE_GLOBS {
            command.arg("--glob").arg(format!("!{glob}"));
        }
    }
    for item in &params.file_pattern_items {
        command.arg("--glob").arg(item);
    }
    command.arg("--");
    command.arg(target);
    command
}

async fn collect_candidates_with_rg(
    root: &Path,
    query: &str,
    query_mode: QueryMode,
    params: &SearchParams,
    unrestricted_paths: bool,
    launch_candidates: &[RgLaunchCandidate],
) -> Result<RgCandidateResult> {
    let (cwd, target) = if root.is_dir() {
        (root.to_path_buf(), PathBuf::from("."))
    } else {
        let parent = root
            .parent()
            .ok_or_else(|| anyhow!("search path has no parent: {}", root.display()))?;
        let target = root
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("search path has invalid file name: {}", root.display()))?;
        (parent.to_path_buf(), target)
    };

    let timeout_window = Duration::from_millis(params.timeout_ms);
    let mut launch_errors = Vec::new();
    for launch in launch_candidates {
        let mut command = build_rg_search_command(
            &launch.program,
            &cwd,
            &target,
            query,
            query_mode,
            params,
            unrestricted_paths,
        );
        let output = match timeout(timeout_window, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                if is_not_found_error(&err) || err.kind() == std::io::ErrorKind::PermissionDenied {
                    launch_errors.push(format!("{} ({err})", launch.display));
                    continue;
                }
                return Err(anyhow!("failed to launch rg via {}: {err}", launch.display));
            }
            Err(_) => return Err(anyhow!("rg timed out after {}ms", params.timeout_ms)),
        };

        match output.status.code() {
            Some(0) => {
                let mut result =
                    parse_rg_candidate_output(&output.stdout, &cwd, params.max_candidates)?;
                result.rg_program = launch.display.clone();
                return Ok(result);
            }
            Some(1) => {
                return Ok(RgCandidateResult {
                    paths: Vec::new(),
                    rg_program: launch.display.clone(),
                    timeout_hit: false,
                    candidate_limit_hit: false,
                });
            }
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let detail = if stderr.is_empty() {
                    "unknown rg runtime error".to_string()
                } else {
                    stderr
                };
                launch_errors.push(format!("{} ({detail})", launch.display));
            }
        }
    }

    if launch_errors.is_empty() {
        return Err(anyhow!("rg executable not available"));
    }
    Err(anyhow!(
        "rg executable not available: {}",
        launch_errors.join("; ")
    ))
}

async fn execute_search_attempt(
    root: &Path,
    params: &SearchParams,
    attempt: &SearchAttempt,
    file_filter: Option<&GlobSet>,
    unrestricted_paths: bool,
    deadline: Instant,
    rg_launch_candidates: &[RgLaunchCandidate],
) -> Result<SearchAttemptResult> {
    let mut resolved_engine = SearchEngine::Rust;
    let mut rg_program: Option<String> = None;
    let mut fallback_reason: Option<String> = None;
    let mut rg_timeout_hit = false;
    let mut candidate_limit_hit = false;
    let mut rg_candidates: Option<Vec<PathBuf>> = None;

    if matches!(params.engine, SearchEngine::Auto | SearchEngine::Rg) {
        match collect_candidates_with_rg(
            root,
            &attempt.query,
            attempt.query_mode,
            params,
            unrestricted_paths,
            rg_launch_candidates,
        )
        .await
        {
            Ok(result) => {
                resolved_engine = SearchEngine::Rg;
                rg_program = Some(result.rg_program);
                rg_timeout_hit = result.timeout_hit;
                candidate_limit_hit = result.candidate_limit_hit;
                rg_candidates = Some(result.paths);
            }
            Err(err) => {
                if params.engine == SearchEngine::Rg {
                    return Err(err);
                }
                fallback_reason = Some(err.to_string());
            }
        }
    }

    let computation = if resolved_engine == SearchEngine::Rg {
        let candidates = rg_candidates.unwrap_or_default();
        let matcher = attempt.matcher.clone();
        let file_filter = file_filter.cloned();
        let root_for_task = root.to_path_buf();
        let params_for_task = params.clone();
        tokio::task::spawn_blocking(move || {
            search_content_with_candidates(
                &root_for_task,
                candidates,
                matcher.as_ref(),
                file_filter.as_ref(),
                &params_for_task,
                unrestricted_paths,
                deadline,
            )
        })
        .await
        .map_err(|err| anyhow!(err.to_string()))??
    } else {
        let matcher = attempt.matcher.clone();
        let file_filter = file_filter.cloned();
        let root_for_task = root.to_path_buf();
        let params_for_task = params.clone();
        tokio::task::spawn_blocking(move || {
            search_content_walk(
                &root_for_task,
                matcher.as_ref(),
                file_filter.as_ref(),
                &params_for_task,
                unrestricted_paths,
                deadline,
            )
        })
        .await
        .map_err(|err| anyhow!(err.to_string()))??
    };

    Ok(SearchAttemptResult {
        hits: computation.hits,
        scanned_files: computation.scanned_files,
        timeout_hit: rg_timeout_hit || computation.timeout_hit,
        file_limit_hit: computation.file_limit_hit,
        match_limit_hit: computation.match_limit_hit,
        resolved_engine,
        rg_program,
        fallback_reason,
        candidate_limit_hit,
    })
}

fn parse_rg_candidate_output(
    stdout: &[u8],
    cwd: &Path,
    max_candidates: usize,
) -> Result<RgCandidateResult> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut candidate_limit_hit = false;

    for line in stdout.split(|item| *item == b'\n') {
        if line.is_empty() {
            continue;
        }
        let item = std::str::from_utf8(line)
            .map_err(|err| anyhow!("rg output is not utf-8: {err}"))?
            .trim();
        if item.is_empty() {
            continue;
        }
        let path = {
            let parsed = PathBuf::from(item);
            if parsed.is_absolute() {
                parsed
            } else {
                cwd.join(parsed)
            }
        };
        let key = path.to_string_lossy().to_string();
        if !seen.insert(key) {
            continue;
        }
        paths.push(path);
        if paths.len() >= max_candidates {
            candidate_limit_hit = true;
            break;
        }
    }

    Ok(RgCandidateResult {
        paths,
        rg_program: String::new(),
        timeout_hit: false,
        candidate_limit_hit,
    })
}

fn search_content_with_candidates(
    root: &Path,
    candidates: Vec<PathBuf>,
    matcher: &Regex,
    file_filter: Option<&GlobSet>,
    params: &SearchParams,
    unrestricted_paths: bool,
    deadline: Instant,
) -> Result<SearchComputation> {
    let display_base = if root.is_dir() {
        root.to_path_buf()
    } else {
        root.parent().unwrap_or(root).to_path_buf()
    };
    let mut hits = Vec::new();
    let mut scanned_files = 0usize;
    let mut timeout_hit = false;
    let mut file_limit_hit = false;
    let mut match_limit_hit = false;

    for candidate in candidates {
        if Instant::now() >= deadline {
            timeout_hit = true;
            break;
        }
        if !unrestricted_paths && tool_fs_filter::should_skip_path(&candidate) {
            continue;
        }
        let rel = candidate
            .strip_prefix(&display_base)
            .unwrap_or(candidate.as_path());
        let rel_display = rel.to_string_lossy().replace('\\', "/");
        if let Some(filter) = file_filter {
            if !filter.is_match(&rel_display) {
                continue;
            }
        }

        scanned_files = scanned_files.saturating_add(1);
        if params.max_files > 0 && scanned_files > params.max_files {
            file_limit_hit = true;
            break;
        }

        let remaining = params.max_matches.saturating_sub(hits.len());
        if remaining == 0 {
            match_limit_hit = true;
            break;
        }

        let local_hits = match search_file(
            &candidate,
            &rel_display,
            matcher,
            params.context_before,
            params.context_after,
            remaining,
        ) {
            Ok(items) => items,
            Err(_) => continue,
        };
        if !local_hits.is_empty() {
            hits.extend(local_hits);
            if hits.len() >= params.max_matches {
                match_limit_hit = true;
                break;
            }
        }
    }

    Ok(SearchComputation {
        hits,
        scanned_files,
        timeout_hit,
        file_limit_hit,
        match_limit_hit,
    })
}

fn search_content_walk(
    root: &Path,
    matcher: &Regex,
    file_filter: Option<&GlobSet>,
    params: &SearchParams,
    unrestricted_paths: bool,
    deadline: Instant,
) -> Result<SearchComputation> {
    let hit_list = Arc::new(Mutex::new(Vec::<SearchHit>::new()));
    let scanned_files = Arc::new(AtomicUsize::new(0));
    let should_stop = Arc::new(AtomicBool::new(false));
    let timeout_hit = Arc::new(AtomicBool::new(false));
    let match_limit_hit = Arc::new(AtomicBool::new(false));
    let file_limit_hit = Arc::new(AtomicBool::new(false));

    let root = Arc::new(root.to_path_buf());
    let display_base = Arc::new(if root.is_dir() {
        root.as_ref().clone()
    } else {
        root.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.as_ref().clone())
    });
    let matcher = Arc::new(matcher.clone());
    let file_filter = file_filter.cloned().map(Arc::new);

    let mut walker = WalkBuilder::new(root.as_ref());
    walker.hidden(false);
    walker.ignore(!unrestricted_paths);
    walker.parents(!unrestricted_paths);
    walker.git_ignore(!unrestricted_paths);
    walker.git_global(!unrestricted_paths);
    walker.git_exclude(!unrestricted_paths);
    walker.max_filesize(Some(MAX_READ_BYTES as u64));
    if params.max_depth > 0 {
        walker.max_depth(Some(params.max_depth));
    }

    walker.build_parallel().run(|| {
        let hit_list = Arc::clone(&hit_list);
        let scanned_files = Arc::clone(&scanned_files);
        let should_stop = Arc::clone(&should_stop);
        let timeout_hit = Arc::clone(&timeout_hit);
        let match_limit_hit = Arc::clone(&match_limit_hit);
        let file_limit_hit = Arc::clone(&file_limit_hit);
        let display_base = Arc::clone(&display_base);
        let matcher = Arc::clone(&matcher);
        let file_filter = file_filter.clone();
        let max_files = params.max_files;
        let max_matches = params.max_matches;
        let context_before = params.context_before;
        let context_after = params.context_after;

        Box::new(move |entry| {
            if should_stop.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            if Instant::now() >= deadline {
                timeout_hit.store(true, Ordering::Relaxed);
                should_stop.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }

            let entry = match entry {
                Ok(value) => value,
                Err(_) => return WalkState::Continue,
            };
            let path = entry.path();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if is_dir {
                if !unrestricted_paths && tool_fs_filter::should_skip_path(path) {
                    return WalkState::Skip;
                }
                return WalkState::Continue;
            }
            if !unrestricted_paths && tool_fs_filter::should_skip_path(path) {
                return WalkState::Continue;
            }

            let rel = path.strip_prefix(display_base.as_ref()).unwrap_or(path);
            let rel_display = rel.to_string_lossy().replace('\\', "/");
            if let Some(filter) = file_filter.as_ref() {
                if !filter.is_match(&rel_display) {
                    return WalkState::Continue;
                }
            }

            let scanned = scanned_files.fetch_add(1, Ordering::Relaxed) + 1;
            if max_files > 0 && scanned > max_files {
                file_limit_hit.store(true, Ordering::Relaxed);
                should_stop.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }

            let remaining = {
                let all_hits = match hit_list.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                max_matches.saturating_sub(all_hits.len())
            };
            if remaining == 0 {
                match_limit_hit.store(true, Ordering::Relaxed);
                should_stop.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }

            let local_hits = match search_file(
                path,
                &rel_display,
                matcher.as_ref(),
                context_before,
                context_after,
                remaining,
            ) {
                Ok(items) => items,
                Err(_) => return WalkState::Continue,
            };
            if local_hits.is_empty() {
                return WalkState::Continue;
            }

            let mut all_hits = match hit_list.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            all_hits.extend(local_hits);
            if all_hits.len() >= max_matches {
                match_limit_hit.store(true, Ordering::Relaxed);
                should_stop.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
            WalkState::Continue
        })
    });

    let hits = match hit_list.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    Ok(SearchComputation {
        hits,
        scanned_files: scanned_files.load(Ordering::Relaxed),
        timeout_hit: timeout_hit.load(Ordering::Relaxed),
        file_limit_hit: file_limit_hit.load(Ordering::Relaxed),
        match_limit_hit: match_limit_hit.load(Ordering::Relaxed),
    })
}

fn search_file(
    path: &Path,
    rel_display: &str,
    matcher: &Regex,
    context_before: usize,
    context_after: usize,
    match_limit: usize,
) -> Result<Vec<SearchHit>> {
    let lines = read_file_lines(path)?;
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if !matcher.is_match(line) {
            continue;
        }
        let before_start = idx.saturating_sub(context_before);
        let after_end = (idx + 1 + context_after).min(lines.len());
        hits.push(SearchHit {
            path: rel_display.to_string(),
            line: idx + 1,
            content: line.to_string(),
            segments: build_highlight_segments(line, matcher),
            matched_terms: Vec::new(),
            before: collect_context_lines(&lines, before_start, idx),
            after: collect_context_lines(&lines, idx + 1, after_end),
        });
        if hits.len() >= match_limit {
            break;
        }
    }
    Ok(hits)
}

fn read_file_lines(path: &Path) -> Result<Vec<String>> {
    if is_probably_binary(path)? {
        return Ok(Vec::new());
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line_buf = Vec::new();
    let mut lines = Vec::new();
    loop {
        line_buf.clear();
        let read = reader.read_until(b'\n', &mut line_buf)?;
        if read == 0 {
            break;
        }
        let line = trim_line_endings(&line_buf);
        lines.push(String::from_utf8_lossy(line).to_string());
    }
    Ok(lines)
}

fn is_probably_binary(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut sample = vec![0u8; BINARY_SAMPLE_BYTES];
    let read = file.read(&mut sample)?;
    sample.truncate(read);
    Ok(looks_like_binary(&sample))
}

fn looks_like_binary(sample: &[u8]) -> bool {
    if sample.is_empty() {
        return false;
    }
    if sample.contains(&0) {
        return true;
    }
    if std::str::from_utf8(sample).is_ok() {
        return false;
    }
    let control_ratio = sample
        .iter()
        .filter(|item| matches!(**item, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1A | 0x1C..=0x1F))
        .count() as f64
        / sample.len() as f64;
    control_ratio >= CONTROL_BYTE_RATIO_THRESHOLD
}

fn collect_context_lines(lines: &[String], start: usize, end: usize) -> Vec<ContextLine> {
    (start..end)
        .map(|idx| ContextLine {
            line: idx + 1,
            content: lines[idx].to_string(),
        })
        .collect::<Vec<_>>()
}

fn build_highlight_segments(line: &str, matcher: &Regex) -> Vec<HighlightSegment> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for matched in matcher.find_iter(line) {
        if matched.start() == matched.end() {
            continue;
        }
        if matched.start() > cursor {
            segments.push(HighlightSegment {
                text: line[cursor..matched.start()].to_string(),
                matched: false,
            });
        }
        segments.push(HighlightSegment {
            text: line[matched.start()..matched.end()].to_string(),
            matched: true,
        });
        cursor = matched.end();
    }
    if cursor < line.len() {
        segments.push(HighlightSegment {
            text: line[cursor..].to_string(),
            matched: false,
        });
    }
    if segments.is_empty() {
        segments.push(HighlightSegment {
            text: line.to_string(),
            matched: false,
        });
    }
    segments
}

fn line_contains_term(line: &str, term: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        return line.contains(term);
    }
    line.to_lowercase().contains(&term.to_lowercase())
}

fn collect_matched_terms_for_line(
    line: &str,
    terms: &[String],
    case_sensitive: bool,
) -> Vec<String> {
    if terms.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    for term in terms {
        if line_contains_term(line, term, case_sensitive) {
            output.push(term.clone());
        }
    }
    output
}

fn hit_line_span(hits: &[SearchHit]) -> Option<(usize, usize)> {
    let mut min_line = usize::MAX;
    let mut max_line = 0usize;
    let mut found = false;
    for hit in hits {
        min_line = min_line.min(hit.line);
        max_line = max_line.max(hit.line);
        found = true;
    }
    if found {
        Some((min_line, max_line))
    } else {
        None
    }
}

fn diversify_hits_within_file(hits: Vec<SearchHit>) -> VecDeque<SearchHit> {
    if hits.len() <= 2 {
        return hits.into();
    }
    let Some((min_line, max_line)) = hit_line_span(&hits) else {
        return hits.into();
    };
    let line_span = max_line.saturating_sub(min_line);
    if line_span < 80 {
        return hits.into();
    }
    let mut remaining = hits;
    let mut diversified = VecDeque::new();
    diversified.push_back(remaining.remove(0));
    while !remaining.is_empty() {
        let mut best_idx = 0usize;
        let mut best_distance = 0usize;
        for (idx, hit) in remaining.iter().enumerate() {
            let distance = diversified
                .iter()
                .map(|picked| hit.line.abs_diff(picked.line))
                .min()
                .unwrap_or(usize::MAX);
            if distance > best_distance {
                best_distance = distance;
                best_idx = idx;
            }
        }
        diversified.push_back(remaining.remove(best_idx));
    }
    diversified
}

fn build_focus_points(hits: &[SearchHit], limit: usize) -> Vec<String> {
    let mut focus_points = Vec::new();
    let mut seen = HashSet::new();
    for hit in hits {
        let key = format!("{}:{}", hit.path, hit.line);
        if !seen.insert(key.clone()) {
            continue;
        }
        let term_suffix = if hit.matched_terms.is_empty() {
            String::new()
        } else {
            let preview_terms = hit
                .matched_terms
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            format!(" [{preview_terms}]")
        };
        focus_points.push(format!("{key}{term_suffix}"));
        if focus_points.len() >= limit {
            break;
        }
    }
    focus_points
}

fn rank_search_hits(
    mut hits: Vec<SearchHit>,
    attempt: &SearchAttempt,
    case_sensitive: bool,
) -> Vec<SearchHit> {
    if hits.is_empty() {
        return hits;
    }
    for hit in &mut hits {
        hit.matched_terms =
            collect_matched_terms_for_line(&hit.content, &attempt.match_terms, case_sensitive);
    }

    let mut file_term_coverage: HashMap<String, HashSet<String>> = HashMap::new();
    for hit in &hits {
        let entry = file_term_coverage.entry(hit.path.clone()).or_default();
        for term in &hit.matched_terms {
            entry.insert(if case_sensitive {
                term.clone()
            } else {
                term.to_lowercase()
            });
        }
    }

    hits.sort_by(|left, right| {
        let left_file_terms = file_term_coverage
            .get(&left.path)
            .map(HashSet::len)
            .unwrap_or(0);
        let right_file_terms = file_term_coverage
            .get(&right.path)
            .map(HashSet::len)
            .unwrap_or(0);
        let left_phrase = attempt
            .preferred_phrase
            .as_deref()
            .map(|phrase| line_contains_term(&left.content, phrase, case_sensitive))
            .unwrap_or(false);
        let right_phrase = attempt
            .preferred_phrase
            .as_deref()
            .map(|phrase| line_contains_term(&right.content, phrase, case_sensitive))
            .unwrap_or(false);
        right_phrase
            .cmp(&left_phrase)
            .then(right_file_terms.cmp(&left_file_terms))
            .then(right.matched_terms.len().cmp(&left.matched_terms.len()))
            .then(left.content.len().cmp(&right.content.len()))
            .then(left.path.cmp(&right.path))
            .then(left.line.cmp(&right.line))
    });

    let mut grouped: HashMap<String, Vec<SearchHit>> = HashMap::new();
    let mut order = Vec::new();
    for hit in hits {
        let key = hit.path.clone();
        let queue = grouped.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Vec::new()
        });
        queue.push(hit);
    }

    let mut grouped = grouped
        .into_iter()
        .map(|(key, hits)| (key, diversify_hits_within_file(hits)))
        .collect::<HashMap<_, _>>();

    for key in &order {
        if let Some(queue) = grouped.get_mut(key) {
            if queue.len() > 1 {
                let mut deduped = VecDeque::new();
                let mut seen_lines = HashSet::new();
                while let Some(hit) = queue.pop_front() {
                    if seen_lines.insert(hit.line) {
                        deduped.push_back(hit);
                    }
                }
                *queue = deduped;
            }
        }
    }

    let mut diversified = Vec::new();
    let mut remaining = true;
    while remaining {
        remaining = false;
        for key in &order {
            if let Some(queue) = grouped.get_mut(key) {
                if let Some(hit) = queue.pop_front() {
                    diversified.push(hit);
                    remaining = true;
                }
            }
        }
    }
    diversified
}

fn build_search_scope(resolved_path: &str) -> Value {
    json!({
        "kind": "workspace_local",
        "local_only": true,
        "supports_web": false,
        "resolved_path": resolved_path,
    })
}

fn build_search_scope_note(resolved_path: &str) -> String {
    format!(
        "Searches local workspace text files only under `{resolved_path}`. This tool does not search the web; use list_files first if the path is uncertain."
    )
}

fn build_zero_hit_search_hint(params: &SearchParams, scanned_files: usize) -> String {
    if scanned_files == 0 {
        return "No readable local text files were scanned. This tool only searches local workspace files, not the web. Use list_files first or widen path/glob.".to_string();
    }
    let fallback_terms = literal_query_fallback_terms(&params.query);
    if !fallback_terms.is_empty() {
        return format!(
            "No exact hit in local workspace files. Also tried term fallback: {}. Narrow path/glob, or switch to pattern/query_mode=regex for structural matching.",
            fallback_terms.join(" | ")
        );
    }
    if params.query_source == QuerySource::Pattern && params.query_mode_inferred {
        return "No hit in local workspace files. pattern defaults to regex; use -F or query_mode=literal for fixed-string matching.".to_string();
    }
    "No hits in local workspace files. Narrow path/glob, or switch query_mode between literal and regex depending on the query shape.".to_string()
}

fn build_search_summary(
    params: &SearchParams,
    attempt: &SearchAttempt,
    attempt_traces: &[SearchAttemptTrace],
    matched_files: &[String],
    hits: &[SearchHit],
    scanned_files: usize,
    output_budget_hit: bool,
) -> SearchSummary {
    let mut matched_terms = Vec::new();
    let mut seen_terms = HashSet::new();
    for hit in hits {
        for term in &hit.matched_terms {
            let key = if params.case_sensitive {
                term.clone()
            } else {
                term.to_lowercase()
            };
            if seen_terms.insert(key) {
                matched_terms.push(term.clone());
            }
        }
    }
    let mut top_files = Vec::new();
    let mut seen_files = HashSet::new();
    for hit in hits {
        if seen_files.insert(hit.path.clone()) {
            top_files.push(hit.path.clone());
        }
        if top_files.len() >= 5 {
            break;
        }
    }
    let focus_points = build_focus_points(hits, 5);
    let single_file_spread = matched_files.len() == 1
        && hits.len() >= 4
        && hit_line_span(hits).is_some_and(|(start, end)| end.saturating_sub(start) >= 200);
    let next_hint = if hits.is_empty() {
        Some(build_zero_hit_search_hint(params, scanned_files))
    } else if output_budget_hit {
        if single_file_spread {
            let (start, end) = hit_line_span(hits).unwrap_or((0, 0));
            Some(format!(
                "Single file matched broadly across lines {start}-{end}. Narrow with a more specific phrase, heading, or smaller anchor before raising output_budget_bytes."
            ))
        } else {
            Some(
                "Result was compacted to fit the output budget. Narrow path/glob or raise output_budget_bytes if you need more hits."
                    .to_string(),
            )
        }
    } else if single_file_spread {
        let (start, end) = hit_line_span(hits).unwrap_or((0, 0));
        Some(format!(
            "Single file matched broadly across lines {start}-{end}. Refine with a more specific phrase, heading, or file-local anchor to avoid linear reading."
        ))
    } else if matched_files.len() > 5
        || attempt_traces
            .iter()
            .any(|trace| trace.returned_match_count > 20)
    {
        Some(
            "Many matches found. Narrow path/glob, reduce context, or raise max_matches only after scoping the search."
                .to_string(),
        )
    } else {
        None
    };

    SearchSummary {
        query_source: params.query_source.as_str().to_string(),
        query_mode: attempt.query_mode.as_str().to_string(),
        query_mode_inferred: params.query_mode_inferred,
        strategy: attempt.strategy.as_str().to_string(),
        fallback_applied: attempt.strategy == SearchStrategy::LiteralTermsFallback,
        returned_match_count: hits.len(),
        matched_file_count: matched_files.len(),
        top_files,
        matched_terms,
        focus_points,
        next_hint,
    }
}

fn estimate_hit_bytes(hit: &SearchHit) -> usize {
    let mut total = hit
        .path
        .len()
        .saturating_add(hit.content.len())
        .saturating_add(64);
    total = total.saturating_add(
        hit.segments
            .iter()
            .map(|segment| segment.text.len().saturating_add(8))
            .sum::<usize>(),
    );
    total = total.saturating_add(
        hit.before
            .iter()
            .map(|line| line.content.len().saturating_add(16))
            .sum::<usize>(),
    );
    total.saturating_add(
        hit.after
            .iter()
            .map(|line| line.content.len().saturating_add(16))
            .sum::<usize>(),
    )
}

fn limit_hits_by_output_budget(
    hits: Vec<SearchHit>,
    output_budget_bytes: Option<usize>,
) -> (Vec<SearchHit>, bool) {
    let Some(output_budget_bytes) = output_budget_bytes else {
        return (hits, false);
    };
    if hits.is_empty() {
        return (hits, false);
    }
    let mut kept = Vec::new();
    let mut used = 0usize;
    for hit in hits {
        let weight = estimate_hit_bytes(&hit).max(1);
        if kept.is_empty() {
            kept.push(hit);
            used = weight.min(output_budget_bytes);
            if weight > output_budget_bytes {
                return (kept, true);
            }
            continue;
        }
        if used.saturating_add(weight) > output_budget_bytes {
            return (kept, true);
        }
        used = used.saturating_add(weight);
        kept.push(hit);
    }
    (kept, false)
}

fn collect_matched_files(hits: &[SearchHit]) -> Vec<String> {
    let mut files = hits
        .iter()
        .map(|item| item.path.clone())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn trim_line_endings(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 {
        let value = bytes[end - 1];
        if value != b'\n' && value != b'\r' {
            break;
        }
        end -= 1;
    }
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn literal_mode_treats_query_as_case_insensitive_literal() {
        let matcher = build_query_matcher("foo.bar", QueryMode::Literal, false).expect("matcher");
        assert!(matcher.is_match("FOO.BAR"));
        assert!(!matcher.is_match("fooXbar"));
    }

    #[test]
    fn regex_mode_keeps_pattern_semantics() {
        let matcher = build_query_matcher(r"foo.+bar", QueryMode::Regex, true).expect("matcher");
        assert!(matcher.is_match("foo123bar"));
        assert!(!matcher.is_match("foobar"));
    }

    #[test]
    fn literal_query_terms_splits_pipe_delimited_items() {
        let terms = literal_query_terms(" alpha | beta|gamma ");
        assert_eq!(terms, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn literal_query_terms_preserves_query_when_only_pipe_delimiters_exist() {
        let terms = literal_query_terms("|");
        assert_eq!(terms, vec!["|"]);
    }

    #[test]
    fn literal_mode_supports_pipe_as_or_separator() {
        let matcher = build_query_matcher("foo|bar", QueryMode::Literal, false).expect("matcher");
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("bar"));
        assert!(!matcher.is_match("baz"));
    }

    #[test]
    fn build_file_filter_rejects_invalid_pattern() {
        let err = build_file_filter(&["[".to_string()]).expect_err("invalid glob");
        assert!(err.to_string().contains("invalid file_pattern"));
    }

    #[test]
    fn file_filter_supports_multiple_globs() {
        let filter = build_file_filter(&["*.rs".to_string(), "*.md".to_string()])
            .expect("filter")
            .expect("set");
        assert!(filter.is_match("src/main.rs"));
        assert!(filter.is_match("docs/README.md"));
        assert!(!filter.is_match("assets/logo.png"));
    }

    #[test]
    fn highlight_segments_mark_match_ranges() {
        let matcher = build_query_matcher("beta", QueryMode::Literal, false).expect("matcher");
        let segments = build_highlight_segments("xx beta yy", &matcher);
        assert_eq!(
            segments,
            vec![
                HighlightSegment {
                    text: "xx ".to_string(),
                    matched: false
                },
                HighlightSegment {
                    text: "beta".to_string(),
                    matched: true
                },
                HighlightSegment {
                    text: " yy".to_string(),
                    matched: false
                }
            ]
        );
    }

    #[test]
    fn search_file_returns_context_and_highlight_segments() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("sample.txt");
        let mut file = File::create(&target).expect("create");
        writeln!(file, "Alpha").expect("write");
        writeln!(file, "beta and BETA").expect("write");
        writeln!(file, "Gamma").expect("write");

        let matcher = build_query_matcher("beta", QueryMode::Literal, false).expect("matcher");
        let hits = search_file(&target, "sample.txt", &matcher, 1, 1, 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].before[0].line, 1);
        assert_eq!(hits[0].after[0].line, 3);
        assert!(hits[0].segments.iter().any(|segment| segment.matched));
    }

    #[test]
    fn parse_rg_candidates_respects_limit_and_dedup() {
        let cwd = Path::new("/tmp");
        let stdout = b"a.txt\na.txt\nb.txt\n";
        let parsed = parse_rg_candidate_output(stdout, cwd, 1).expect("parse");
        assert_eq!(parsed.paths.len(), 1);
        assert!(parsed.candidate_limit_hit);
    }

    #[test]
    fn parse_search_params_applies_budget_caps() {
        let params = parse_search_params(&json!({
            "query": "foo",
            "max_matches": 200,
            "timeout_ms": 30000,
            "max_candidates": 5000,
            "max_files": 100,
            "budget": {
                "time_budget_ms": 1200,
                "max_matches": 20,
                "max_candidates": 80,
                "max_files": 10,
                "output_budget_bytes": 4096
            }
        }))
        .expect("params");
        assert_eq!(params.timeout_ms, 1200);
        assert_eq!(params.max_matches, 20);
        assert_eq!(params.max_candidates, 80);
        assert_eq!(params.max_files, 10);
        assert_eq!(params.output_budget_bytes, Some(4096));
    }

    #[test]
    fn parse_search_params_accepts_rg_style_aliases() {
        let params = parse_search_params(&json!({
            "pattern": "turn_terminal|event title",
            "glob": "*.rs,*.md",
            "type": "rust,markdown",
            "-B": 1,
            "-A": 4,
            "-i": true,
            "head_limit": 25
        }))
        .expect("params");
        assert_eq!(params.query, "turn_terminal|event title");
        assert_eq!(params.query_mode, QueryMode::Regex);
        assert!(params.query_mode_inferred);
        assert_eq!(params.file_pattern_items, vec!["*.rs", "*.md", "*.mdx"]);
        assert!(!params.case_sensitive);
        assert_eq!(params.context_before, 1);
        assert_eq!(params.context_after, 4);
        assert_eq!(params.max_matches, 25);
    }

    #[test]
    fn parse_query_mode_supports_fixed_strings_aliases() {
        let params = parse_search_params(&json!({
            "pattern": "foo.bar",
            "-F": true
        }))
        .expect("params");
        assert_eq!(params.query_mode, QueryMode::Literal);
        assert!(!params.query_mode_inferred);
    }

    #[test]
    fn literal_query_fallback_terms_split_model_style_queries() {
        let terms =
            literal_query_fallback_terms("alphaDoc betaRef gammaNode deltaPoint epsilonTag");
        assert_eq!(
            terms,
            vec![
                "alphaDoc",
                "betaRef",
                "gammaNode",
                "deltaPoint",
                "epsilonTag"
            ]
        );
    }

    #[test]
    fn build_search_attempts_adds_literal_terms_fallback_for_query() {
        let params = parse_search_params(&json!({
            "query": "alphaDoc betaRef gammaNode deltaPoint epsilonTag"
        }))
        .expect("params");
        let attempts = build_search_attempts(&params).expect("attempts");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].strategy, SearchStrategy::LiteralExact);
        assert_eq!(attempts[1].strategy, SearchStrategy::LiteralTermsFallback);
        assert_eq!(
            attempts[1].match_terms,
            vec![
                "alphaDoc",
                "betaRef",
                "gammaNode",
                "deltaPoint",
                "epsilonTag"
            ]
        );
    }

    #[test]
    fn collect_matched_files_sorts_and_deduplicates_paths() {
        let files = collect_matched_files(&[
            SearchHit {
                path: "src/b.rs".to_string(),
                line: 2,
                content: "beta".to_string(),
                segments: vec![],
                matched_terms: vec![],
                before: vec![],
                after: vec![],
            },
            SearchHit {
                path: "src/a.rs".to_string(),
                line: 1,
                content: "alpha".to_string(),
                segments: vec![],
                matched_terms: vec![],
                before: vec![],
                after: vec![],
            },
            SearchHit {
                path: "src/b.rs".to_string(),
                line: 5,
                content: "beta two".to_string(),
                segments: vec![],
                matched_terms: vec![],
                before: vec![],
                after: vec![],
            },
        ]);
        assert_eq!(files, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    }

    #[test]
    fn limit_hits_by_output_budget_truncates_hits() {
        let hit1 = SearchHit {
            path: "a.rs".to_string(),
            line: 1,
            content: "alpha".to_string(),
            segments: vec![],
            matched_terms: vec![],
            before: vec![],
            after: vec![],
        };
        let hit2 = SearchHit {
            path: "b.rs".to_string(),
            line: 2,
            content: "beta".to_string(),
            segments: vec![],
            matched_terms: vec![],
            before: vec![],
            after: vec![],
        };
        let one_weight = estimate_hit_bytes(&hit1);
        let (hits, budget_hit) =
            limit_hits_by_output_budget(vec![hit1, hit2], Some(one_weight + 1));
        assert_eq!(hits.len(), 1);
        assert!(budget_hit);
    }

    #[test]
    fn rank_search_hits_prioritizes_broader_term_coverage() {
        let attempt = build_search_attempt(
            SearchStrategy::LiteralTermsFallback,
            "alpha|beta|gamma".to_string(),
            QueryMode::Literal,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            Some("alpha beta gamma".to_string()),
            false,
        )
        .expect("attempt");
        let ranked = rank_search_hits(
            vec![
                SearchHit {
                    path: "b.md".to_string(),
                    line: 10,
                    content: "alpha".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
                SearchHit {
                    path: "a.md".to_string(),
                    line: 3,
                    content: "alpha beta".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
                SearchHit {
                    path: "a.md".to_string(),
                    line: 5,
                    content: "gamma".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
            ],
            &attempt,
            false,
        );
        assert_eq!(ranked[0].path, "a.md");
        assert_eq!(ranked[0].content, "alpha beta");
    }

    #[test]
    fn rank_search_hits_diversifies_single_file_regions() {
        let attempt = build_search_attempt(
            SearchStrategy::LiteralTermsFallback,
            "alpha".to_string(),
            QueryMode::Literal,
            vec!["alpha".to_string()],
            Some("alpha".to_string()),
            false,
        )
        .expect("attempt");
        let ranked = rank_search_hits(
            vec![
                SearchHit {
                    path: "a.md".to_string(),
                    line: 10,
                    content: "alpha one".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
                SearchHit {
                    path: "a.md".to_string(),
                    line: 20,
                    content: "alpha two".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
                SearchHit {
                    path: "a.md".to_string(),
                    line: 300,
                    content: "alpha three".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
                SearchHit {
                    path: "a.md".to_string(),
                    line: 900,
                    content: "alpha four".to_string(),
                    segments: vec![],
                    matched_terms: vec![],
                    before: vec![],
                    after: vec![],
                },
            ],
            &attempt,
            false,
        );
        let first_three = ranked
            .iter()
            .take(3)
            .map(|hit| hit.line)
            .collect::<Vec<_>>();
        assert_eq!(first_three, vec![10, 900, 300]);
    }

    #[test]
    fn build_search_summary_marks_broad_single_file_match() {
        let params = SearchParams {
            query: "alpha".to_string(),
            query_source: QuerySource::Query,
            path: ".".to_string(),
            file_pattern_items: vec!["*.md".to_string()],
            query_mode: QueryMode::Literal,
            query_mode_inferred: false,
            case_sensitive: false,
            context_before: 0,
            context_after: 0,
            max_depth: 0,
            max_files: 0,
            max_matches: 50,
            max_candidates: 4000,
            timeout_ms: 30_000,
            engine: SearchEngine::Rust,
            output_budget_bytes: Some(4096),
        };
        let attempt = build_search_attempt(
            SearchStrategy::LiteralExact,
            "alpha".to_string(),
            QueryMode::Literal,
            vec!["alpha".to_string()],
            Some("alpha".to_string()),
            false,
        )
        .expect("attempt");
        let hits = vec![
            SearchHit {
                path: "a.md".to_string(),
                line: 15,
                content: "alpha".to_string(),
                segments: vec![],
                matched_terms: vec!["alpha".to_string()],
                before: vec![],
                after: vec![],
            },
            SearchHit {
                path: "a.md".to_string(),
                line: 250,
                content: "alpha".to_string(),
                segments: vec![],
                matched_terms: vec!["alpha".to_string()],
                before: vec![],
                after: vec![],
            },
            SearchHit {
                path: "a.md".to_string(),
                line: 700,
                content: "alpha".to_string(),
                segments: vec![],
                matched_terms: vec!["alpha".to_string()],
                before: vec![],
                after: vec![],
            },
            SearchHit {
                path: "a.md".to_string(),
                line: 1200,
                content: "alpha".to_string(),
                segments: vec![],
                matched_terms: vec!["alpha".to_string()],
                before: vec![],
                after: vec![],
            },
        ];
        let summary = build_search_summary(
            &params,
            &attempt,
            &[SearchAttemptTrace {
                strategy: "literal_exact".to_string(),
                query_mode: "literal".to_string(),
                query_used: "alpha".to_string(),
                returned_match_count: 4,
                matched_file_count: 1,
                scanned_files: 1,
            }],
            &["a.md".to_string()],
            &hits,
            1,
            true,
        );
        assert_eq!(summary.focus_points[0], "a.md:15 [alpha]");
        assert!(summary
            .next_hint
            .as_deref()
            .unwrap_or("")
            .contains("Single file matched broadly across lines 15-1200"));
    }

    #[test]
    fn zero_hit_summary_explicitly_marks_local_scope() {
        let params = SearchParams {
            query: "alpha beta".to_string(),
            query_source: QuerySource::Query,
            path: ".".to_string(),
            file_pattern_items: vec![],
            query_mode: QueryMode::Literal,
            query_mode_inferred: false,
            case_sensitive: false,
            context_before: 0,
            context_after: 0,
            max_depth: 0,
            max_files: 0,
            max_matches: 20,
            max_candidates: DEFAULT_MAX_CANDIDATES,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            engine: SearchEngine::Auto,
            output_budget_bytes: None,
        };
        let attempt = build_search_attempt(
            SearchStrategy::LiteralExact,
            "alpha beta".to_string(),
            QueryMode::Literal,
            vec!["alpha beta".to_string()],
            Some("alpha beta".to_string()),
            false,
        )
        .expect("attempt");
        let summary = build_search_summary(&params, &attempt, &[], &[], &[], 0, false);
        assert!(summary
            .next_hint
            .as_deref()
            .unwrap_or("")
            .contains("local workspace files"));
        assert!(summary
            .next_hint
            .as_deref()
            .unwrap_or("")
            .contains("not the web"));
    }

    #[test]
    fn push_rg_candidate_reference_resolves_relative_path_from_base_dirs() {
        let dir = tempdir().expect("tempdir");
        let relative = Path::new("opt/rg").join(rg_binary_name());
        let target = dir.path().join(&relative);
        std::fs::create_dir_all(
            target
                .parent()
                .expect("relative target should have parent directory"),
        )
        .expect("create dir");
        File::create(&target).expect("create rg binary");

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        push_rg_candidate_reference(
            &mut candidates,
            &mut seen,
            &relative.to_string_lossy(),
            &[dir.path().to_path_buf()],
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(PathBuf::from(&candidates[0].display), target);
    }

    #[test]
    fn push_rg_candidate_reference_treats_single_token_as_program() {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        push_rg_candidate_reference(&mut candidates, &mut seen, "rg-custom", &[]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].display, "rg-custom");
    }
}
