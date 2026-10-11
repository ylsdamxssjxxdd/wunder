//! Bulk synthetic thread generation for message-rendering stress tests (SQLite).
//!
//! Writes the same SQLite schema the runtime reads, but bypasses the per-event
//! commit path on purpose: one bulk connection with relaxed journal settings
//! and one transaction per user turn. Rows are buffered and flushed as
//! multi-row `INSERT ... VALUES (?,?,...)` statements (2000 rows per statement
//! stays well under the 32766 bound-parameter limit of bundled SQLite; the
//! statement is cached so repeated full batches skip re-parsing), which
//! is the difference between minutes and hours at the default 1000×1000 scale.
//! Timestamps are simulated backwards from "now" so the generated thread looks
//! like it happened in the recent past. Sampling lives in the shared,
//! storage-agnostic `stress_model` module.

use super::SqliteStorage;
use crate::storage::stress_model::{
    base_seed, build_answer_text, build_reasoning_text, build_tool_args, build_tool_result,
    committed_item_json, plan_timeline, profile_hint, sample_turn_plans, IdGen, StressThreadSpec,
    StressThreadStats,
};
use anyhow::{ensure, Result};
use chrono::{Local, TimeZone};
use rusqlite::{params, params_from_iter, Connection, ToSql, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use std::time::Duration;
use wunder_core::storage_backend::StorageLifecycle;

/// Rows per multi-row INSERT. 2000 × 13 = 26000 parameters, still under the
/// 32766 bind-parameter limit of bundled SQLite, and fewer statements per turn.
const BATCH_ROWS: usize = 2000;

/// A buffered parameter value for bulk inserts.
enum P {
    S(String),
    I(i64),
    F(f64),
    N(Option<String>),
}

impl ToSql for P {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            P::S(v) => v.to_sql(),
            P::I(v) => v.to_sql(),
            P::F(v) => v.to_sql(),
            P::N(None) => rusqlite::types::Null.to_sql(),
            P::N(Some(v)) => v.to_sql(),
        }
    }
}

fn multirow_sql(table: &str, columns: &str, row_width: usize, rows: usize) -> String {
    let mut sql = String::with_capacity(96 + rows * row_width * 4);
    sql.push_str("INSERT INTO ");
    sql.push_str(table);
    sql.push('(');
    sql.push_str(columns);
    sql.push_str(") VALUES ");
    for r in 0..rows {
        if r > 0 {
            sql.push(',');
        }
        sql.push('(');
        for c in 0..row_width {
            if c > 0 {
                sql.push(',');
            }
            sql.push('?');
        }
        sql.push(')');
    }
    sql
}

const ITEM_COLUMNS: &str = "session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq";
const CHANGE_COLUMNS: &str = "session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time";
const TOOL_LOG_COLUMNS: &str = "user_id,session_id,tool,ok,error,args,data,timestamp,payload,created_time";

fn flush_batch(
    tx: &Transaction<'_>,
    table: &str,
    columns: &str,
    row_width: usize,
    rows: &mut Vec<Vec<P>>,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let sql = multirow_sql(table, columns, row_width, rows.len());
    // Reuse a cached prepared statement: the full-size batch SQL text repeats
    // across flushes, so SQLite skips re-parsing a 26k-placeholder INSERT.
    let mut stmt = tx.prepare_cached(&sql)?;
    stmt.execute(params_from_iter(rows.iter().flatten()))?;
    rows.clear();
    Ok(())
}

impl SqliteStorage {
    /// Generate one complete synthetic thread in bulk. Progress is reported
    /// once per finished user turn as (done_rounds, items_written).
    pub fn generate_stress_thread(
        &self,
        user_id: &str,
        spec: &StressThreadSpec,
        mut progress: impl FnMut(i64, i64) + Send,
    ) -> Result<StressThreadStats> {
        self.ensure_initialized()?;
        ensure!(!user_id.trim().is_empty(), "missing user identity");

        let seed = base_seed();
        let now = Self::now_ts();
        // Pass 1: sample the whole timeline so the simulated clock can end at
        // "now"; the thread then reads as recent history.
        let plan = plan_timeline(seed, now, spec)?;

        // Bulk connection: journal in memory, no fsync per commit. WAL is
        // restored afterwards so the live runtime keeps its normal settings.
        let mut conn = Connection::open(&self.db_path)?;
        conn.busy_timeout(Duration::from_secs(30)).ok();
        conn.pragma_update(None, "journal_mode", "MEMORY").ok();
        conn.pragma_update(None, "synchronous", "OFF").ok();
        conn.pragma_update(None, "cache_size", -262_144).ok();

        let write_result = self.write_stress_thread(
            &mut conn,
            user_id,
            spec,
            &plan,
            seed,
            &mut progress,
        );
        let stats = match write_result {
            Ok(stats) => stats,
            Err(err) => {
                // Remove the partial thread so no half-rendered session lingers.
                let _ = conn.execute_batch(&format!(
                    "BEGIN; \
                     DELETE FROM thread_items WHERE session_id='{sid}'; \
                     DELETE FROM thread_turns WHERE session_id='{sid}'; \
                     DELETE FROM thread_item_blocks WHERE session_id='{sid}'; \
                     DELETE FROM thread_log_changes WHERE session_id='{sid}'; \
                     DELETE FROM thread_log_metrics WHERE session_id='{sid}'; \
                     DELETE FROM tool_logs WHERE session_id='{sid}'; \
                     DELETE FROM thread_logs WHERE session_id='{sid}'; \
                     DELETE FROM chat_sessions WHERE session_id='{sid}'; \
                     COMMIT;",
                    sid = spec.session_id.replace('\'', "''")
                ));
                return Err(err);
            }
        };

        conn.pragma_update(None, "journal_mode", "WAL").ok();
        Ok(stats)
    }

    fn write_stress_thread(
        &self,
        conn: &mut Connection,
        user_id: &str,
        spec: &StressThreadSpec,
        plan: &crate::storage::stress_model::StressTimelinePlan,
        seed: u64,
        progress: &mut impl FnMut(i64, i64),
    ) -> Result<StressThreadStats> {
        let session_id = spec.session_id.as_str();
        let mut seq: i64 = 0;
        let mut clock = plan.start_time;
        let mut items_written: i64 = 0;
        let mut tool_calls_total: i64 = 0;
        let mut turn_rng = crate::storage::stress_model::Rng::new(0xC0FF_EE00 ^ seed);
        let mut ids = IdGen::new(seed);

        // Buffered rows: item / change / tool_log batches are flushed inside
        // the current turn transaction and whenever they reach BATCH_ROWS.
        let mut item_rows: Vec<Vec<P>> = Vec::with_capacity(BATCH_ROWS);
        let mut change_rows: Vec<Vec<P>> = Vec::with_capacity(BATCH_ROWS);
        let mut tool_rows: Vec<Vec<P>> = Vec::with_capacity(BATCH_ROWS);

        for (offset, cost) in plan.costs.iter().enumerate() {
            let turn_index = offset as i64 + 1;
            clock += cost.gap_before_s;
            let turn_start = clock;
            let turn_id = ids.next().to_string();
            let client_message_id = format!("stress-{}", ids.next().simple());
            let prompt_template = turn_rng.pick_str(crate::storage::stress_model::user_prompts());
            let prompt = crate::storage::stress_model::fill_template(prompt_template, &mut turn_rng, turn_index);

            // Same per-turn seed as pass 1: identical tool counts/durations.
            let plans = sample_turn_plans(cost.turn_seed, turn_index, spec.model_rounds_per_turn);

            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Turn row (terminal state, like a settled real turn).
            tx.execute(
                "INSERT INTO thread_turns(session_id,turn_id,root_turn_id,trigger_kind,client_message_id,user_id,user_turn_index,status,summary,payload,created_time,updated_time) \
                 VALUES(?,?,?,?,?,?,?,'completed',?,'{}',?,?)",
                params![
                    session_id,
                    turn_id,
                    turn_id,
                    "user",
                    client_message_id,
                    user_id,
                    turn_index,
                    prompt.chars().take(240).collect::<String>(),
                    turn_start,
                    turn_start,
                ],
            )?;
            // turn_upsert change row mirrors accept_thread_turn.
            seq += 1;
            if seq > plan.change_tail_cutoff {
                change_rows.push(vec![
                    P::S(session_id.to_string()),
                    P::I(seq),
                    P::S(user_id.to_string()),
                    P::S("turn_upsert".into()),
                    P::S(turn_id.clone()),
                    P::N(None),
                    P::I(1),
                    P::S(serde_json::to_string(&json!({
                        "turn_id": turn_id,
                        "root_turn_id": turn_id,
                        "trigger_kind": "user",
                        "status": "queued",
                        "user_round": turn_index,
                        "client_message_id": client_message_id,
                    }))?),
                    P::F(turn_start),
                ]);
            }

            // User bubble item.
            let user_item_id = format!("{turn_id}:user");
            let user_payload = serde_json::to_string(&json!({
                "role": "user",
                "content": prompt,
                "client_message_id": client_message_id,
                "session_id": session_id,
                "turn_id": turn_id,
                "user_round": turn_index,
                "item_id": user_item_id,
                "status": "completed",
            }))?;
            item_rows.push(vec![
                P::S(session_id.to_string()),
                P::S(user_item_id.clone()),
                P::S(turn_id.clone()),
                P::S(turn_id.clone()),
                P::S("user".into()),
                P::S(user_id.to_string()),
                P::I(0),
                P::S("user_message".into()),
                P::S("completed".into()),
                P::S(user_payload.clone()),
                P::F(turn_start),
                P::F(turn_start),
                P::I(seq),
            ]);
            items_written += 1;
            if item_rows.len() >= BATCH_ROWS {
                flush_batch(&tx, "thread_items", ITEM_COLUMNS, 13, &mut item_rows)?;
            }
            seq += 1;
            if seq > plan.change_tail_cutoff {
                change_rows.push(vec![
                    P::S(session_id.to_string()),
                    P::I(seq),
                    P::S(user_id.to_string()),
                    P::S("item_upsert".into()),
                    P::S(turn_id.clone()),
                    P::S(user_item_id.clone()),
                    P::I(1),
                    P::S(committed_item_json(
                        &user_item_id,
                        0,
                        "user_message",
                        "completed",
                        &user_payload,
                        turn_start,
                        turn_start,
                        &turn_id,
                        seq,
                    )?),
                    P::F(turn_start),
                ]);
            }

            let mut item_index: i64 = 0;
            let mut turn_clock = turn_start;

            for (m, plan_round) in plans.iter().enumerate() {
                let model_round = m as i64 + 1;
                // Per-round factor fits the sampled round cost exactly (plans
                // are identical to pass 1, so the factor is ~1.0 and only
                // absorbs floating-point drift).
                let raw_round = plan_round.prefill_duration_s
                    + plan_round.decode_duration_s
                    + plan_round.tool_duration_s.iter().sum::<f64>();
                let scale = cost.round_costs[m] / raw_round.max(1e-9);
                turn_clock += plan_round.prefill_duration_s * scale;
                let round_start = turn_clock;

                let mut call_profiles: Vec<(&crate::storage::stress_model::ToolProfile, f64)> = Vec::new();
                for (i, profile) in plan_round.tool_profiles.iter().enumerate() {
                    call_profiles.push((*profile, plan_round.tool_duration_s[i] * scale));
                }
                let tool_done_at = round_start + call_profiles.iter().map(|(_, d)| *d).sum::<f64>();

                for (profile, duration_s) in &call_profiles {
                    let call_id = format!("call_{}", ids.next().simple());
                    let args = build_tool_args(&mut turn_rng, profile, turn_index);
                    let args_text = serde_json::to_string(&args)?;
                    let duration_ms = (*duration_s * 1000.0).round() as i64;
                    let result = build_tool_result(&mut turn_rng, profile, &args, duration_ms)?;
                    let failed = result.failed;
                    let tool_done = round_start + duration_s;
                    let item_id = format!("{turn_id}:tool-{call_id}");
                    let usage = json!({
                        "input_tokens": plan_round.input_tokens,
                        "output_tokens": plan_round.answer_tokens,
                        "total_tokens": plan_round.input_tokens + plan_round.answer_tokens,
                        "reasoning_tokens": plan_round.reasoning_tokens.unwrap_or(0),
                        "estimated": true,
                    });
                    let mut payload = json!({
                        "tool": profile.name,
                        "ok": !failed,
                        "data": result.data,
                        "args": args,
                        "request_context_tokens": plan_round.input_tokens,
                        "request_usage": usage,
                        "tool_runtime_name": profile.name,
                        "tool_display_name": profile.name,
                        "tool_function_name": profile.function_name,
                        "tool_call_id": call_id,
                        "model_observation": { "tool": profile.name, "ok": !failed },
                        "event_type": "tool_result",
                        "session_id": session_id,
                        "item_id": item_id,
                        "kind": "tool_call",
                        "status": if failed { "failed" } else { "completed" },
                        "user_round": turn_index,
                        "model_round": model_round,
                        "turn_id": turn_id,
                    });
                    if !result.error.is_empty() {
                        payload["error"] = json!(&result.error);
                    }
                    payload["meta"] = result.meta;
                    let payload_text = serde_json::to_string(&payload)?;
                    item_index += 1;
                    seq += 1;
                    item_rows.push(vec![
                        P::S(session_id.to_string()),
                        P::S(item_id.clone()),
                        P::S(turn_id.clone()),
                        P::S(turn_id.clone()),
                        P::S("user".into()),
                        P::S(user_id.to_string()),
                        P::I(item_index),
                        P::S("tool_call".into()),
                        P::S(if failed { "failed".into() } else { "completed".to_string() }),
                        P::S(payload_text.clone()),
                        P::F(tool_done),
                        P::F(tool_done),
                        P::I(seq),
                    ]);
                    items_written += 1;
                    tool_calls_total += 1;
                    if item_rows.len() >= BATCH_ROWS {
                        flush_batch(&tx, "thread_items", ITEM_COLUMNS, 13, &mut item_rows)?;
                    }
                    if seq > plan.change_tail_cutoff {
                        change_rows.push(vec![
                            P::S(session_id.to_string()),
                            P::I(seq),
                            P::S(user_id.to_string()),
                            P::S("item_upsert".into()),
                            P::S(turn_id.clone()),
                            P::S(item_id.clone()),
                            P::I(1),
                            P::S(committed_item_json(
                                &item_id,
                                item_index,
                                "tool_call",
                                if failed { "failed" } else { "completed" },
                                &payload_text,
                                tool_done,
                                tool_done,
                                &turn_id,
                                seq,
                            )?),
                            P::F(tool_done),
                        ]);
                    }
                    // tool_logs mirrors append_tool_log's payload column.
                    tool_rows.push(vec![
                        P::S(user_id.to_string()),
                        P::S(session_id.to_string()),
                        P::S(profile.name.to_string()),
                        P::I(if failed { 0 } else { 1 }),
                        P::S(result.error),
                        P::S(args_text),
                        P::S(result.data_text),
                        P::S(Local
                            .timestamp_millis_opt((tool_done * 1000.0) as i64)
                            .single()
                            .map(|t| t.to_rfc3339())
                            .unwrap_or_default()),
                        P::S(payload_text),
                        P::F(tool_done),
                    ]);
                    if change_rows.len() >= BATCH_ROWS {
                        flush_batch(&tx, "thread_log_changes", CHANGE_COLUMNS, 9, &mut change_rows)?;
                    }
                    if tool_rows.len() >= BATCH_ROWS {
                        flush_batch(&tx, "tool_logs", TOOL_LOG_COLUMNS, 10, &mut tool_rows)?;
                    }
                }

                // Assistant message closes the round.
                turn_clock = tool_done_at + plan_round.decode_duration_s * scale;
                let done_at = turn_clock;
                let decode_output_tokens = plan_round.answer_tokens;
                let usage = json!({
                    "input_tokens": plan_round.input_tokens,
                    "output_tokens": plan_round.answer_tokens + plan_round.reasoning_tokens.unwrap_or(0),
                    "total_tokens": plan_round.input_tokens + plan_round.answer_tokens + plan_round.reasoning_tokens.unwrap_or(0),
                    "reasoning_tokens": plan_round.reasoning_tokens.unwrap_or(0),
                    "estimated": true,
                });
                let answer = if m as i64 + 1 == spec.model_rounds_per_turn {
                    build_answer_text(&mut turn_rng, &prompt, turn_index)
                } else {
                    format!("继续处理：{}（第 {} 轮）", profile_hint(&call_profiles), model_round)
                };
                let reasoning = plan_round
                    .reasoning_tokens
                    .map(|tokens| build_reasoning_text(&mut turn_rng, tokens))
                    .unwrap_or_default();
                let prefill_ms = (plan_round.prefill_duration_s * scale * 1000.0).round() as u64;
                let decode_ms = (plan_round.decode_duration_s * scale * 1000.0).round() as u64;
                let content_chars = answer.chars().count();
                let payload = serde_json::to_string(&json!({
                    "content": answer,
                    "reasoning": reasoning,
                    "usage": usage,
                    "decode_output_tokens": decode_output_tokens,
                    "tool_calls": [],
                    "prefill_duration_s": plan_round.prefill_duration_s * scale,
                    "decode_duration_s": plan_round.decode_duration_s * scale,
                    "stream_timing": {
                        "chunk_count": (decode_output_tokens / 3).max(8),
                        "content_delta_chars": content_chars,
                        "reasoning_delta_chars": reasoning.chars().count(),
                        "prefill_ms": prefill_ms,
                        "decode_ms": decode_ms,
                        "content_decode_ms": decode_ms,
                        "max_chunk_gap_ms": turn_rng.int(20, 600),
                    },
                    "ttft_ms": prefill_ms,
                    "prefill_tokens": plan_round.input_tokens,
                    "prefill_speed_tps": (plan_round.input_tokens as f64 / (plan_round.prefill_duration_s * scale).max(1e-6)),
                    "prefill_speed_lower_bound": 0,
                    "decode_tokens": decode_output_tokens + plan_round.reasoning_tokens.unwrap_or(0),
                    "decode_speed_tps": ((decode_output_tokens + plan_round.reasoning_tokens.unwrap_or(0)) as f64
                        / (plan_round.decode_duration_s * scale).max(1e-6)),
                    "decode_stream_chunk_tokens": turn_rng.int(2, 6),
                    "finish_reason": "stop",
                    "output_limit_reached": false,
                    "max_output": 32768,
                    "thinking_token_budget": Value::Null,
                    "thinking_disabled": false,
                    "event_type": "llm_output",
                    "session_id": session_id,
                    "item_id": format!("{turn_id}:text-{model_round}"),
                    "kind": "assistant_message",
                    "status": "completed",
                    "user_round": turn_index,
                    "model_round": model_round,
                    "turn_id": turn_id,
                }))?;
                item_index += 1;
                seq += 1;
                let item_id = format!("{turn_id}:text-{model_round}");
                item_rows.push(vec![
                    P::S(session_id.to_string()),
                    P::S(item_id.clone()),
                    P::S(turn_id.clone()),
                    P::S(turn_id.clone()),
                    P::S("user".into()),
                    P::S(user_id.to_string()),
                    P::I(item_index),
                    P::S("assistant_message".into()),
                    P::S("completed".into()),
                    P::S(payload.clone()),
                    P::F(done_at),
                    P::F(done_at),
                    P::I(seq),
                ]);
                items_written += 1;
                if item_rows.len() >= BATCH_ROWS {
                    flush_batch(&tx, "thread_items", ITEM_COLUMNS, 13, &mut item_rows)?;
                }
                if seq > plan.change_tail_cutoff {
                    change_rows.push(vec![
                        P::S(session_id.to_string()),
                        P::I(seq),
                        P::S(user_id.to_string()),
                        P::S("item_upsert".into()),
                        P::S(turn_id.clone()),
                        P::S(item_id.clone()),
                        P::I(1),
                        P::S(committed_item_json(
                            &item_id,
                            item_index,
                            "assistant_message",
                            "completed",
                            &payload,
                            done_at,
                            done_at,
                            &turn_id,
                            seq,
                        )?),
                        P::F(done_at),
                    ]);
                }
            }

            // Terminal turn_upsert mirrors update_thread_turn's final change.
            let turn_end = turn_clock;
            seq += 1;
            if seq > plan.change_tail_cutoff {
                change_rows.push(vec![
                    P::S(session_id.to_string()),
                    P::I(seq),
                    P::S(user_id.to_string()),
                    P::S("turn_upsert".into()),
                    P::S(turn_id.clone()),
                    P::N(None),
                    P::I(seq),
                    P::S(serde_json::to_string(&json!({
                        "turn_id": turn_id,
                        "status": "completed",
                        "root_turn_id": turn_id,
                        "trigger_kind": "user",
                        "user_round": turn_index,
                    }))?),
                    P::F(turn_end),
                ]);
            }
            flush_batch(&tx, "thread_items", ITEM_COLUMNS, 13, &mut item_rows)?;
            flush_batch(&tx, "thread_log_changes", CHANGE_COLUMNS, 9, &mut change_rows)?;
            flush_batch(&tx, "tool_logs", TOOL_LOG_COLUMNS, 10, &mut tool_rows)?;
            tx.execute(
                "UPDATE thread_turns SET updated_time=? WHERE session_id=? AND turn_id=?",
                params![turn_end, session_id, turn_id],
            )?;
            tx.commit()?;
            progress(turn_index, items_written);
        }

        // Final envelope: session registration, log header, metrics. One short
        // transaction; chat_sessions appears last so the list only ever shows a
        // complete thread.
        let end_time = clock;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES(?,?,?,?,?,?)",
            params![session_id, user_id, spec.user_rounds, seq, plan.start_time, end_time],
        )?;
        tx.execute(
            "INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) VALUES(?,?, 'user_turn_total', ?, ?) \
             ON CONFLICT(session_id,metric_key) DO UPDATE SET metric_value=excluded.metric_value,updated_time=excluded.updated_time",
            params![session_id, user_id, spec.user_rounds, end_time],
        )?;
        tx.execute(
            "INSERT INTO chat_sessions(session_id,user_id,title,status,created_at,updated_at,last_message_at) VALUES(?,?,?,'active',?,?,?)",
            params![session_id, user_id, spec.title, plan.start_time, end_time, end_time],
        )?;
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=? AND change_seq<=?",
            params![session_id, plan.change_tail_cutoff],
        )?;
        tx.commit()?;

        Ok(StressThreadStats {
            session_id: spec.session_id.clone(),
            user_turns: spec.user_rounds,
            items: items_written,
            tool_calls: tool_calls_total,
        })
    }
}

#[cfg(all(test, feature = "sqlite-storage"))]
mod tests {
    use super::*;
    use crate::storage::stress_model::MAX_USER_ROUNDS;
    use uuid::Uuid;
    use wunder_core::storage_backend::{ChatSessionStore, ThreadLogStore};

    fn temp_db() -> (std::path::PathBuf, SqliteStorage) {
        let dir = std::env::temp_dir().join(format!("wunder-stress-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("wunder.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        (dir, storage)
    }

    #[test]
    fn generated_thread_is_readable_through_normal_paths() {
        let (dir, storage) = temp_db();
        let spec = StressThreadSpec {
            session_id: "stress-session-1".to_string(),
            title: "渲染压测 3×4".to_string(),
            user_rounds: 3,
            model_rounds_per_turn: 4,
        };
        let mut progress_rounds = Vec::new();
        let stats = storage
            .generate_stress_thread("tester", &spec, |done, _| progress_rounds.push(done))
            .unwrap();
        assert_eq!(stats.user_turns, 3);
        assert!(stats.tool_calls > 0);
        assert!(stats.items > 3 * 4);
        assert_eq!(progress_rounds.len(), 3);

        // Session appears in the catalog.
        let (sessions, _) = storage
            .list_chat_sessions("tester", None, None, 0, 50)
            .unwrap();
        assert!(sessions
            .iter()
            .any(|s| s.session_id == "stress-session-1"));

        // Visible messages: every model round contributes an assistant bubble,
        // so 3 turns × (1 user + 4 assistant) = 15, newest first.
        let visible = storage
            .list_thread_visible_messages("tester", "stress-session-1", None, 500)
            .unwrap();
        assert_eq!(visible.len(), 15);
        let roles: Vec<&str> = visible
            .iter()
            .map(|m| m["role"].as_str().unwrap_or_default())
            .collect();
        // Newest first: each turn ends with its 4th assistant bubble, so the
        // reversed timeline reads [assistant×4, user] per turn.
        let mut expected: Vec<&str> = Vec::new();
        for _ in 0..3 {
            for _ in 0..4 {
                expected.push("assistant");
            }
            expected.push("user");
        }
        assert_eq!(roles, expected);
        for message in &visible {
            assert_eq!(message["status"], json!("completed"));
        }

        // Turn listing and turn detail both resolve with terminal items.
        let turns = storage
            .list_thread_turns("tester", "stress-session-1", None, 50)
            .unwrap();
        assert_eq!(turns.len(), 3);
        let turn_id = turns[0]["turn_id"].as_str().unwrap().to_string();
        assert_eq!(turns[0]["status"], json!("completed"));
        let detail = storage
            .get_thread_turn("tester", "stress-session-1", &turn_id, -1, 200, false)
            .unwrap()
            .unwrap();
        let items = detail["items"].as_array().unwrap();
        // user bubble + at least one item per model round (tools + assistant)
        assert!(items.len() >= 5, "turn items: {}", items.len());
        let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap_or_default()).collect();
        assert_eq!(kinds.first().copied(), Some("user_message"));
        assert!(kinds.contains(&"tool_call"));
        assert!(kinds.contains(&"assistant_message"));
        let tool_item = items.iter().find(|i| i["kind"] == json!("tool_call")).unwrap();
        let payload = &tool_item["payload"];
        assert!(payload["tool_call_id"].as_str().is_some());
        assert!(payload["tool"].as_str().is_some());
        assert!(payload["args"].is_object());
        assert!(payload["request_usage"].is_object());
        let assistant = items
            .iter()
            .find(|i| i["kind"] == json!("assistant_message"))
            .unwrap();
        let payload = &assistant["payload"];
        assert!(payload["content"].as_str().is_some());
        assert!(payload["usage"]["input_tokens"].as_u64().unwrap() > 0);
        assert!(payload["ttft_ms"].as_u64().unwrap() > 0);
        assert!(payload["prefill_speed_tps"].as_f64().unwrap() > 1000.0);
        assert_eq!(payload["event_type"], json!("llm_output"));

        // Simulated timestamps are strictly increasing and end before now.
        let first_created = items[0]["created_time"].as_f64().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        assert!(first_created < now, "timestamps must be in the past");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_requests_are_rejected() {
        let (dir, storage) = temp_db();
        let spec = StressThreadSpec {
            session_id: "stress-too-big".to_string(),
            title: "超出上限".to_string(),
            user_rounds: MAX_USER_ROUNDS + 1,
            model_rounds_per_turn: 4,
        };
        assert!(storage.generate_stress_thread("tester", &spec, |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore] // perf probe: run explicitly with -- --ignored
    fn mid_size_generation_perf() {
        let (dir, storage) = temp_db();
        let spec = StressThreadSpec {
            session_id: "stress-perf-50x20".to_string(),
            title: "渲染压测 50×20".to_string(),
            user_rounds: 50,
            model_rounds_per_turn: 20,
        };
        let started = std::time::Instant::now();
        let stats = storage
            .generate_stress_thread("tester", &spec, |done, items| {
                if done % 10 == 0 {
                    println!("progress rounds={done} items={items}");
                }
            })
            .unwrap();
        let elapsed = started.elapsed();
        println!(
            "generated 50x20: items={} tool_calls={} elapsed={elapsed:?} ({:.0} items/s)",
            stats.items,
            stats.tool_calls,
            stats.items as f64 / elapsed.as_secs_f64().max(1e-9)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore] // micro A/B: cached prepared stmt vs one-shot execute, interleaved
    fn prepared_vs_raw_flush_micro() {
        use wunder_core::storage_backend::StorageLifecycle;
        let (dir, storage) = temp_db();
        storage.ensure_initialized().unwrap();
        let mut conn = Connection::open(storage.db_path.clone()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(30)).ok();
        conn.pragma_update(None, "journal_mode", "MEMORY").ok();
        conn.pragma_update(None, "synchronous", "OFF").ok();

        const COLUMNS: &str = "session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq";
        let n = BATCH_ROWS;
        let sql = multirow_sql("thread_items", COLUMNS, 13, n);
        let payload = "x".repeat(1024);
        let make_rows = |tag: &str| -> Vec<Vec<P>> {
            let session = format!("micro-{tag}");
            let mut rows: Vec<Vec<P>> = Vec::with_capacity(n);
            for i in 0..n {
                rows.push(vec![
                    P::S(session.clone()),
                    P::S(format!("{tag}-item-{i:07}")),
                    P::S(format!("{tag}-turn")),
                    P::S(format!("{tag}-turn")),
                    P::S("user".to_string()),
                    P::S("micro-user".to_string()),
                    P::I(i as i64),
                    P::S("assistant_message".to_string()),
                    P::S("completed".to_string()),
                    P::S(payload.clone()),
                    P::F(1_700_000_000.0),
                    P::F(1_700_000_000.0),
                    P::I(seq_of(i)),
                ]);
            }
            rows
        };
        fn seq_of(i: usize) -> i64 {
            i as i64
        }

        let mut raw = std::time::Duration::ZERO;
        let mut cached = std::time::Duration::ZERO;
        for round in 0..8 {
            {
                let tx = conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .unwrap();
                let mut rows = make_rows(&format!("raw{round}"));
                let t = std::time::Instant::now();
                tx.execute(&sql, params_from_iter(rows.iter().flatten()))
                    .unwrap();
                raw += t.elapsed();
                rows.clear();
                tx.commit().unwrap();
            }
            {
                let tx = conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .unwrap();
                let mut rows = make_rows(&format!("cached{round}"));
                let t = std::time::Instant::now();
                {
                    let mut stmt = tx.prepare_cached(&sql).unwrap();
                    stmt.execute(params_from_iter(rows.iter().flatten())).unwrap();
                }
                cached += t.elapsed();
                rows.clear();
                tx.commit().unwrap();
            }
        }
        println!("micro flush {n}x13 x8: raw={raw:?} cached={cached:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
