//! Local trajectory I/O. All presentation reads use the engine's bounded views.
use crate::args::{GlobalArgs, TrajectoryCommand};
use crate::runtime::CliRuntime;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncWriteExt, BufWriter};
use wunder_server::stable_core::thread_view::{BodyRef, ThreadViewPage, TrajectoryViewRow};
use wunder_server::storage::StorageBackend;

pub(crate) const DETAIL_BYTES: usize = 1024 * 1024;
const DETAIL_LINES: usize = 16_384;

/// Closing a reader cannot abort a database call already running. Keep permits
/// inside blocking jobs so reopening readers cannot flood the blocking pool.
pub(crate) async fn read<T, F>(work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    static LIMIT: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let permit = LIMIT
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await?
}

#[derive(Default)]
pub(crate) struct Detail {
    pub value: Value,
    pub truncated: bool,
    pub sections: Vec<Vec<String>>,
}

/// Cursors have different units for projected and fallback bodies. Only advance
/// using the cursor returned by storage; never infer it from string length.
fn read_body(
    store: &dyn StorageBackend,
    owner: &str,
    session: &str,
    body: &BodyRef,
    budget: &mut usize,
) -> Result<(String, bool)> {
    let mut text = String::new();
    let mut cursor = 0;
    loop {
        let page = store.thread_body_page(owner, session, &body.body_id, body.revision, cursor)?;
        for chunk in page.chunks {
            let end = chunk.len().min(*budget);
            let mut end = end;
            while !chunk.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&chunk[..end]);
            *budget -= end;
            if end < chunk.len() {
                return Ok((text, true));
            }
        }
        if !page.has_more {
            return Ok((text, false));
        }
        if *budget == 0 {
            return Ok((text, true));
        }
        cursor = page
            .next_cursor
            .filter(|next| *next > cursor)
            .ok_or_else(|| anyhow!("body cursor did not advance"))?;
    }
}

pub(crate) fn load_detail(
    store: &dyn StorageBackend,
    owner: &str,
    session: &str,
    row: &TrajectoryViewRow,
) -> Result<Detail> {
    let Some(body) = &row.detail else {
        return Ok(Detail::default());
    };
    let mut budget = DETAIL_BYTES;
    let (raw, mut truncated) = read_body(store, owner, session, body, &mut budget)?;
    let mut value = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
    if let Some(map) = value.as_object_mut() {
        map.remove("model_content");
        map.remove("config_overrides");
    }
    // Message content is stored separately to keep the trajectory projection small.
    if let Some(reference) = value.get("content_body").cloned() {
        let reference: BodyRef = serde_json::from_value(reference)?;
        let (content, more) = read_body(store, owner, session, &reference, &mut budget)?;
        value["content"] = Value::String(content);
        truncated |= more;
    }
    if row.kind == "assistant_message" && value.is_object() {
        if value["content"].as_str().is_none_or(str::is_empty) {
            let source = BodyRef {
                body_id: format!("{}|source_stream", row.item_id),
                revision: row.revision,
                bytes: 0,
            };
            let (content, more) = read_body(store, owner, session, &source, &mut budget)?;
            value["content"] = Value::String(content);
            truncated |= more;
        }
        if value["reasoning"].as_str().is_none_or(str::is_empty)
            && value["reasoning_content"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            let mut cursor = 0;
            let mut text = String::new();
            loop {
                let (blocks, next, more) = store.list_thread_item_blocks_page(
                    owner,
                    session,
                    &row.item_id,
                    Some("reasoning"),
                    cursor,
                    16,
                    false,
                )?;
                for block in blocks {
                    let data = block.get("data").unwrap_or(&block);
                    let part = data["reasoning"]
                        .as_str()
                        .or(data["reasoning_content"].as_str())
                        .unwrap_or("");
                    let original_bytes = part.len();
                    let part = wunder_server::stable_core::thread_view::preview(part, budget);
                    truncated |= part.len() < original_bytes;
                    budget -= part.len();
                    text.push_str(&part);
                }
                if !more || truncated {
                    break;
                }
                if budget == 0 {
                    truncated = true;
                    break;
                }
                cursor = next
                    .map(|n| n.saturating_add(1))
                    .filter(|n| *n > cursor)
                    .ok_or_else(|| anyhow!("reasoning cursor did not advance"))?;
            }
            value["reasoning"] = Value::String(text);
        }
    }
    let sections = detail_sections(row, &value, truncated);
    Ok(Detail {
        value,
        truncated,
        sections,
    })
}

fn pretty(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string_pretty(v).unwrap_or_default(),
        None => String::new(),
    }
}

fn detail_sections(row: &TrajectoryViewRow, value: &Value, truncated: bool) -> Vec<Vec<String>> {
    let first = |keys: &[&str]| keys.iter().find_map(|key| value.get(*key));
    let overview = format!(
        "{}\n{} · {}\nturn: {} / model: {}\nitem: {}\n{}\n{}",
        row.tool_name.as_deref().unwrap_or(&row.kind),
        row.kind,
        row.status,
        row.user_round,
        row.model_round,
        row.item_id,
        row.summary,
        pretty(first(&["failure", "error", "diagnostics"]))
    );
    let content = if value.is_string() {
        pretty(Some(value))
    } else {
        pretty(first(&["content", "summary", "message", "runtime", "text"]))
    };
    let reasoning = pretty(first(&["reasoning", "reasoning_content"]));
    let preview = if reasoning.is_empty() {
        content
    } else {
        format!("{content}\n\nReasoning / 思考\n{reasoning}")
    };
    let timing = format!(
        "start: {:.3}\nend: {}\nelapsed: {}\nttft_ms: {}\nstats:\n{}",
        row.started_at,
        row.completed_at
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "running".into()),
        row.completed_at
            .map(|v| format!("{:.3}s", (v - row.started_at).max(0.)))
            .unwrap_or_else(|| "—".into()),
        pretty(value.get("ttft_ms")),
        pretty(row.stats.as_ref().or(value.get("message_stats")))
    );
    [overview, preview, pretty(Some(value)), pretty(first(&["args", "arguments", "input", "task"])),
        pretty(first(&["result", "output", "data", "content", "runtime"])), pretty(first(&["schema", "tool_schema"])), timing]
        .into_iter().map(|text| {
            let mut source = text.lines();
            let mut lines: Vec<String> = source.by_ref().take(DETAIL_LINES).map(str::to_owned).collect();
            if source.next().is_some() { lines.push("[Line preview limit reached; export for full content / 超出行数预览上限，请导出完整日志]".into()); }
            if truncated { lines.insert(0, "[Preview limited to 1 MiB; export for the full log / 预览上限 1 MiB，可导出完整日志]".into()); }
            lines
        }).collect()
}

pub(crate) fn public_item_row(
    store: &dyn StorageBackend,
    owner: &str,
    session: &str,
    id: &str,
) -> Result<TrajectoryViewRow> {
    let item = store
        .get_thread_item(owner, session, id, false)?
        .ok_or_else(|| anyhow!("item not found / 未找到条目"))?;
    let payload = &item["payload"];
    let revision = item["revision"].as_i64().unwrap_or(0);
    let turn_id = item["turn_id"].as_str().unwrap_or("");
    let turn = store.get_thread_turn(owner, session, turn_id, -1, 1, false)?;
    Ok(TrajectoryViewRow {
        item_id: id.into(),
        revision,
        turn_id: turn_id.into(),
        root_turn_id: turn
            .as_ref()
            .and_then(|t| t["root_turn_id"].as_str())
            .unwrap_or(turn_id)
            .into(),
        user_round: turn
            .as_ref()
            .and_then(|t| t["user_turn_index"].as_i64())
            .unwrap_or(0),
        kind: item["kind"].as_str().unwrap_or("search_hit").into(),
        status: item["status"].as_str().unwrap_or("").into(),
        model_round: payload["model_round"].as_i64().unwrap_or(0),
        tool_name: payload["tool"].as_str().map(str::to_owned),
        started_at: item["created_time"].as_f64().unwrap_or(0.),
        completed_at: (!matches!(
            item["status"].as_str(),
            Some("running" | "waiting_input" | "queued")
        ))
        .then(|| item["updated_time"].as_f64())
        .flatten(),
        detail: Some(BodyRef {
            body_id: format!("{id}|source_detail"),
            revision,
            bytes: 0,
        }),
        ..Default::default()
    })
}

pub(crate) async fn export(
    runtime: &CliRuntime,
    session: String,
    path: PathBuf,
) -> Result<PathBuf> {
    // create_new prevents accidental replacement, including symlink targets.
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .context("create trajectory export (the destination must not exist)")?;
    let mut output = BufWriter::new(file);
    let mut stream = wunder_server::export_thread_log_stream(
        runtime.state.storage.clone(),
        runtime.user_id.clone(),
        session,
        false,
    );
    while let Some(record) = stream.recv().await {
        output.write_all(record?.as_bytes()).await?;
    }
    output.flush().await?;
    Ok(path)
}

pub(crate) async fn handle(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    cmd: TrajectoryCommand,
) -> Result<()> {
    let session = match cmd.session_id.as_deref().or(global.session.as_deref()) {
        Some(id) => id.to_owned(),
        None => crate::list_recent_sessions_in(runtime, 1, None, Some(runtime.workspace_id()))
            .await?
            .first()
            .map(|s| s.session_id.clone())
            .ok_or_else(|| anyhow!("no thread to inspect / 没有可查看的线程"))?,
    };
    if let Some(path) = cmd.export {
        if path == Path::new("-") {
            let mut stream = wunder_server::export_thread_log_stream(
                runtime.state.storage.clone(),
                runtime.user_id.clone(),
                session,
                false,
            );
            let mut output = BufWriter::new(tokio::io::stdout());
            while let Some(record) = stream.recv().await {
                output.write_all(record?.as_bytes()).await?;
            }
            output.flush().await?;
        } else {
            let path = export(runtime, session, path).await?;
            eprintln!("{}", path.display());
        }
        return Ok(());
    }
    let store = runtime.state.storage.clone();
    let user = runtime.user_id.clone();
    if let Some(query) = cmd.search {
        let found =
            crate::trajectory::read(move || store.thread_view_search(&user, &session, &query))
                .await?;
        if global.json || cmd.raw {
            println!("{}", serde_json::to_string_pretty(&found)?);
        } else {
            for hit in found.hits {
                println!(
                    "{}  {}",
                    hit.item_id,
                    hit.snippet.replace(['\r', '\n'], " ")
                );
            }
            if found.has_more {
                eprintln!("20+ hits; refine query / 超过 20 个命中，请缩小搜索范围");
            }
        }
        return Ok(());
    }
    let item = cmd.item;
    let result = crate::trajectory::read(move || -> Result<Value> {
        if let Some(item_id) = item {
            let row = public_item_row(store.as_ref(), &user, &session, &item_id)?;
            let detail = load_detail(store.as_ref(), &user, &session, &row)?;
            return Ok(
                json!({"item_id": item_id, "detail": detail.value, "truncated": detail.truncated}),
            );
        }
        let page = store.thread_view_window(
            &user,
            &session,
            "trajectory",
            cmd.before,
            cmd.after,
            cmd.turn.as_deref(),
            cmd.limit.clamp(1, 100),
        )?;
        Ok(serde_json::to_value(page)?)
    })
    .await?;
    if global.json || cmd.raw || result.get("detail").is_some() {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let page: ThreadViewPage = serde_json::from_value(result)?;
        println!("seq   turn  kind                 status      summary");
        for r in page.trajectory {
            println!(
                "{:>5} {:>5} {:<20} {:<11} {}",
                r.created_seq,
                r.user_round,
                r.kind,
                r.status,
                r.summary.replace(['\r', '\n'], " ")
            );
        }
        if page.has_more {
            eprintln!("--before {}", page.next_before.unwrap_or(0));
        }
        if page.has_newer {
            eprintln!("--after {}", page.next_after.unwrap_or(0));
        }
        if let Some(diagnostic) = page.diagnostic {
            eprintln!("{diagnostic}");
        }
        if page.degraded {
            bail!("trajectory view is degraded / 轨迹视图暂时降级，请重试");
        }
    }
    Ok(())
}
