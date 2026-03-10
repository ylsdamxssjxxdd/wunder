use super::{
    collect_read_roots, resolve_tool_path, ToolContext, MAX_READ_BYTES, MAX_SEARCH_MATCHES,
};
use crate::core::tool_fs_filter;
use crate::i18n;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAX_CONTEXT_LINES: usize = 20;

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
    before: Vec<ContextLine>,
    after: Vec<ContextLine>,
}

pub(super) async fn search_content(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.search.empty")
        }));
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string();
    let file_pattern = args
        .get("file_pattern")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let query_mode = parse_query_mode(args);
    let case_sensitive = args
        .get("case_sensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let context_before =
        normalize_context_window(args.get("context_before").and_then(Value::as_u64));
    let context_after = normalize_context_window(args.get("context_after").and_then(Value::as_u64));
    let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(0) as usize;
    let max_files = args.get("max_files").and_then(Value::as_u64).unwrap_or(0) as usize;

    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    tokio::task::spawn_blocking(move || {
        search_content_inner(
            workspace.as_ref(),
            &user_id,
            &query,
            &path,
            &file_pattern,
            query_mode,
            case_sensitive,
            context_before,
            context_after,
            &extra_roots,
            max_depth,
            max_files,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?
}

#[allow(clippy::too_many_arguments)]
fn search_content_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    query: &str,
    path: &str,
    file_pattern: &str,
    query_mode: QueryMode,
    case_sensitive: bool,
    context_before: usize,
    context_after: usize,
    extra_roots: &[PathBuf],
    max_depth: usize,
    max_files: usize,
) -> Result<Value> {
    let root = resolve_tool_path(workspace, user_id, path, extra_roots)?;
    if !root.exists() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.search.path_not_found")
        }));
    }

    let matcher = match build_query_matcher(query, query_mode, case_sensitive) {
        Ok(value) => Arc::new(value),
        Err(err) => {
            return Ok(json!({
                "ok": false,
                "data": {},
                "error": err.to_string()
            }));
        }
    };
    let file_filter = Arc::new(build_file_filter(file_pattern));
    let hit_list = Arc::new(Mutex::new(Vec::<SearchHit>::new()));
    let scanned_files = Arc::new(AtomicUsize::new(0));
    let should_stop = Arc::new(AtomicBool::new(false));
    let match_limit_hit = Arc::new(AtomicBool::new(false));
    let file_limit_hit = Arc::new(AtomicBool::new(false));
    let root = Arc::new(root);

    let mut walker = WalkBuilder::new(root.as_ref());
    walker.hidden(false);
    walker.ignore(true);
    walker.parents(true);
    walker.git_ignore(true);
    walker.git_global(true);
    walker.git_exclude(true);
    walker.max_filesize(Some(MAX_READ_BYTES as u64));
    if max_depth > 0 {
        walker.max_depth(Some(max_depth));
    }

    // Keep search parallel and fast while preserving deterministic output by sorting at the end.
    walker.build_parallel().run(|| {
        let matcher = Arc::clone(&matcher);
        let file_filter = Arc::clone(&file_filter);
        let hit_list = Arc::clone(&hit_list);
        let scanned_files = Arc::clone(&scanned_files);
        let should_stop = Arc::clone(&should_stop);
        let match_limit_hit = Arc::clone(&match_limit_hit);
        let file_limit_hit = Arc::clone(&file_limit_hit);
        let root = Arc::clone(&root);
        Box::new(move |entry| {
            if should_stop.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            let entry = match entry {
                Ok(value) => value,
                Err(_) => return WalkState::Continue,
            };
            let path = entry.path();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if is_dir {
                if tool_fs_filter::should_skip_path(path) {
                    return WalkState::Skip;
                }
                return WalkState::Continue;
            }
            if tool_fs_filter::should_skip_path(path) {
                return WalkState::Continue;
            }

            let scanned = scanned_files.fetch_add(1, Ordering::Relaxed) + 1;
            if max_files > 0 && scanned > max_files {
                file_limit_hit.store(true, Ordering::Relaxed);
                should_stop.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }

            let rel = path.strip_prefix(root.as_ref()).unwrap_or(path);
            let rel_display = rel.to_string_lossy().replace('\\', "/");
            if let Some(filter) = file_filter.as_ref() {
                if !filter.is_match(&rel_display) {
                    return WalkState::Continue;
                }
            }

            let local_hits = match search_file(
                path,
                &rel_display,
                matcher.as_ref(),
                context_before,
                context_after,
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
            if all_hits.len() >= MAX_SEARCH_MATCHES {
                match_limit_hit.store(true, Ordering::Relaxed);
                should_stop.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
            WalkState::Continue
        })
    });

    let mut hit_list = match hit_list.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    hit_list.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.content.cmp(&right.content))
    });
    if hit_list.len() > MAX_SEARCH_MATCHES {
        hit_list.truncate(MAX_SEARCH_MATCHES);
    }

    // Keep legacy `matches` for backward compatibility while adding rich `hits`.
    let matches = hit_list
        .iter()
        .map(|item| format!("{}:{}:{}", item.path, item.line, item.content.trim()))
        .collect::<Vec<_>>();
    Ok(json!({
        "matches": matches,
        "hits": hit_list,
        "query_mode": query_mode.as_str(),
        "case_sensitive": case_sensitive,
        "context_before": context_before,
        "context_after": context_after,
        "scanned_files": scanned_files.load(Ordering::Relaxed),
        "file_limit_hit": file_limit_hit.load(Ordering::Relaxed),
        "match_limit_hit": match_limit_hit.load(Ordering::Relaxed)
    }))
}

fn parse_query_mode(args: &Value) -> QueryMode {
    if let Some(raw) = args.get("query_mode").and_then(Value::as_str) {
        let cleaned = raw.trim().to_ascii_lowercase();
        if cleaned == "regex" || cleaned == "re" {
            return QueryMode::Regex;
        }
    }
    if args.get("regex").and_then(Value::as_bool).unwrap_or(false) {
        return QueryMode::Regex;
    }
    QueryMode::Literal
}

fn normalize_context_window(raw: Option<u64>) -> usize {
    raw.unwrap_or(0).min(MAX_CONTEXT_LINES as u64) as usize
}

fn build_query_matcher(query: &str, query_mode: QueryMode, case_sensitive: bool) -> Result<Regex> {
    let pattern = match query_mode {
        QueryMode::Literal => regex::escape(query),
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

fn build_file_filter(raw: &str) -> Option<GlobSet> {
    let items = raw
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    let mut builder = GlobSetBuilder::new();
    for item in items {
        let glob = Glob::new(item).ok()?;
        builder.add(glob);
    }
    builder.build().ok()
}

fn search_file(
    path: &Path,
    rel_display: &str,
    matcher: &Regex,
    context_before: usize,
    context_after: usize,
) -> Result<Vec<SearchHit>> {
    let lines = read_file_lines(path)?;
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
            before: collect_context_lines(&lines, before_start, idx),
            after: collect_context_lines(&lines, idx + 1, after_end),
        });
        if hits.len() >= MAX_SEARCH_MATCHES {
            break;
        }
    }
    Ok(hits)
}

fn read_file_lines(path: &Path) -> Result<Vec<String>> {
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
    fn file_filter_supports_multiple_globs() {
        let filter = build_file_filter("*.rs,*.md").expect("filter");
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
        let hits = search_file(&target, "sample.txt", &matcher, 1, 1).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].before[0].line, 1);
        assert_eq!(hits[0].after[0].line, 3);
        assert!(hits[0].segments.iter().any(|segment| segment.matched));
    }
}
