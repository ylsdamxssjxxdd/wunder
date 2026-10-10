//! Bulk synthetic thread generation for message-rendering stress tests (PostgreSQL).
//!
//! Mirrors the SQLite bulk writer: one dedicated pooled connection, one
//! transaction per user turn, simulated timestamps ending at "now". Rows are
//! buffered and flushed as multi-row `INSERT ... VALUES (...)` statements
//! (800 rows per statement keeps the 65535-parameter protocol limit at bay),
//! which is the difference between minutes and hours at the default 1000×1000
//! scale. Sampling logic is shared with the SQLite backend in `stress_model`.

use super::PostgresStorage;
use crate::storage::stress_model::{
    base_seed, build_answer_text, build_reasoning_text, build_tool_args, build_tool_result,
    committed_item_json, fill_template, plan_timeline, profile_hint, sample_turn_plans, user_prompts,
    Rng, StressThreadSpec, StressThreadStats, StressTimelinePlan,
};
use anyhow::{anyhow, ensure, Result};
use chrono::{Local, TimeZone};
use serde_json::{json, Value};
use tokio_postgres::types::ToSql;
use tokio_postgres::Transaction;
use uuid::Uuid;
use wunder_core::storage_backend::StorageLifecycle;

/// Rows per multi-row INSERT. 800 × 13 = 10400 parameters, well under the
/// 65535 protocol limit, and a comfortable payload size per round-trip.
const BATCH_ROWS: usize = 800;

/// A buffered parameter value for bulk inserts.
enum P {
    S(String),
    I(i64),
    I32(i32),
    F(f64),
    N(Option<String>),
}

impl P {
    fn as_sql(&self) -> &(dyn ToSql + Sync) {
        match self {
            P::S(v) => v,
            P::I(v) => v,
            P::I32(v) => v,
            P::F(v) => v,
            P::N(v) => v,
        }
    }
}

fn multirow_sql(table: &str, columns: &str, row_width: usize, rows: usize) -> String {
    let mut sql = String::with_capacity(96 + rows * row_width * 6);
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
            sql.push_str(&format!("${}", r * row_width + c + 1));
        }
        sql.push(')');
    }
    sql
}

async fn flush_batch(
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
    let flat: Vec<&(dyn ToSql + Sync)> =
        rows.iter().flat_map(|row| row.iter().map(P::as_sql)).collect();
    tx.execute(&sql, &flat).await?;
    rows.clear();
    Ok(())
}

impl PostgresStorage {
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
        let plan = plan_timeline(seed, Self::now_ts(), spec)?;
        // The whole generation is one async job driven by block_on: the caller
        // is either a blocking thread (job service) or a runtime worker with
        // block_in_place, so blocking here is safe. The outer Result only
        // covers runtime/pool failures; the inner one is the generation result.
        let result = self.block_on(generate_async(
            self,
            user_id,
            spec,
            &plan,
            seed,
            &mut progress,
        ))?;
        result
    }
}

async fn generate_async(
    storage: &PostgresStorage,
    user_id: &str,
    spec: &StressThreadSpec,
    plan: &StressTimelinePlan,
    seed: u64,
    progress: &mut impl FnMut(i64, i64),
) -> Result<StressThreadStats> {
    let mut client = storage
        .pool
        .get()
        .await
        .map_err(|err| anyhow!("acquire postgres connection: {err}"))?;
    match run_generation(&mut client, user_id, spec, plan, seed, progress).await {
        Ok(stats) => Ok(stats),
        Err(err) => {
            cleanup_partial(&mut client, &spec.session_id).await;
            Err(err)
        }
    }
}

async fn cleanup_partial(client: &mut deadpool_postgres::Client, session_id: &str) {
    // Per-table deletes ignore failures so a missing optional table (e.g.
    // thread_item_blocks) cannot mask the original error.
    for table in [
        "thread_items",
        "thread_turns",
        "thread_item_blocks",
        "thread_log_changes",
        "thread_log_metrics",
        "tool_logs",
        "thread_logs",
        "chat_sessions",
    ] {
        let _ = client
            .execute(
                &format!("DELETE FROM {table} WHERE session_id=$1"),
                &[&session_id],
            )
            .await;
    }
}

async fn run_generation(
    client: &mut deadpool_postgres::Client,
    user_id: &str,
    spec: &StressThreadSpec,
    plan: &StressTimelinePlan,
    seed: u64,
    progress: &mut impl FnMut(i64, i64),
) -> Result<StressThreadStats> {
    let session_id = spec.session_id.as_str();
    let mut seq: i64 = 0;
    let mut clock = plan.start_time;
    let mut items_written: i64 = 0;
    let mut tool_calls_total: i64 = 0;
    let mut turn_rng = Rng::new(0xC0FF_EE00 ^ seed);

    // Buffered rows: item / change / tool_log batches are flushed per turn
    // transaction and whenever they reach BATCH_ROWS.
    let mut item_rows: Vec<Vec<P>> = Vec::with_capacity(BATCH_ROWS);
    let mut change_rows: Vec<Vec<P>> = Vec::with_capacity(BATCH_ROWS);
    let mut tool_rows: Vec<Vec<P>> = Vec::with_capacity(BATCH_ROWS);

    for (offset, cost) in plan.costs.iter().enumerate() {
        let turn_index = offset as i64 + 1;
        clock += cost.gap_before_s;
        let turn_start = clock;
        let turn_id = Uuid::new_v4().to_string();
        let client_message_id = format!("stress-{}", Uuid::new_v4().simple());
        let prompt = fill_template(turn_rng.pick_str(user_prompts()), &mut turn_rng, turn_index);
        let plans = sample_turn_plans(cost.turn_seed, turn_index, spec.model_rounds_per_turn);

        let tx = client
            .transaction()
            .await
            .map_err(|err| anyhow!("begin turn transaction: {err}"))?;
        // Turn row (terminal state, like a settled real turn).
        tx.execute(
            "INSERT INTO thread_turns(session_id,turn_id,root_turn_id,trigger_kind,client_message_id,user_id,user_turn_index,status,summary,payload,created_time,updated_time) \
             VALUES($1,$2,$3,'user',$4,$5,$6,'completed',$7,'{}',$8,$9)",
            &[
                &session_id,
                &turn_id,
                &turn_id,
                &client_message_id,
                &user_id,
                &turn_index,
                &prompt.chars().take(240).collect::<String>(),
                &turn_start,
                &turn_start,
            ],
        )
        .await?;
        seq += 1;
        if seq > plan.change_tail_cutoff {
            change_rows.push(vec![
                P::S(session_id.to_string()),
                P::I(seq),
                P::S(user_id.to_string()),
                P::S(turn_id.to_string()),
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
        if change_rows.len() >= BATCH_ROWS {
            flush_batch(&tx, "thread_log_changes", "session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time", 9, &mut change_rows).await?;
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
            P::S("user".to_string()),
            P::S(user_id.to_string()),
            P::I(0),
            P::S("user_message".to_string()),
            P::S("completed".to_string()),
            P::S(user_payload.clone()),
            P::F(turn_start),
            P::F(turn_start),
            P::I(seq),
        ]);
        items_written += 1;
        seq += 1;
        if seq > plan.change_tail_cutoff {
            change_rows.push(vec![
                P::S(session_id.to_string()),
                P::I(seq),
                P::S(user_id.to_string()),
                P::S(turn_id.to_string()),
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
        if item_rows.len() >= BATCH_ROWS {
            flush_batch(&tx, "thread_items", "session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq", 13, &mut item_rows).await?;
        }
        if change_rows.len() >= BATCH_ROWS {
            flush_batch(&tx, "thread_log_changes", "session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time", 9, &mut change_rows).await?;
        }

        let mut item_index: i64 = 0;
        let mut turn_clock = turn_start;

        for (m, plan_round) in plans.iter().enumerate() {
            let model_round = m as i64 + 1;
            let raw_round = plan_round.prefill_duration_s
                + plan_round.decode_duration_s
                + plan_round.tool_duration_s.iter().sum::<f64>();
            let scale = cost.round_costs[m] / raw_round.max(1e-9);
            turn_clock += plan_round.prefill_duration_s * scale;
            let round_start = turn_clock;

            let mut call_profiles: Vec<(&crate::storage::stress_model::ToolProfile, f64)> =
                Vec::new();
            for (i, profile) in plan_round.tool_profiles.iter().enumerate() {
                call_profiles.push((*profile, plan_round.tool_duration_s[i] * scale));
            }
            let tool_done_at = round_start + call_profiles.iter().map(|(_, d)| *d).sum::<f64>();

            for (profile, duration_s) in &call_profiles {
                let call_id = format!("call_{}", Uuid::new_v4().simple());
                let args = build_tool_args(&mut turn_rng, profile, turn_index);
                let duration_ms = (*duration_s * 1000.0).round() as i64;
                let (failed, data, error, meta) =
                    build_tool_result(&mut turn_rng, profile, &args, duration_ms);
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
                    "data": data,
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
                if !error.is_empty() {
                    payload["error"] = json!(error);
                }
                payload["meta"] = meta;
                let payload_text = serde_json::to_string(&payload)?;
                item_index += 1;
                seq += 1;
                item_rows.push(vec![
                    P::S(session_id.to_string()),
                    P::S(item_id.clone()),
                    P::S(turn_id.clone()),
                    P::S(turn_id.clone()),
                    P::S("user".to_string()),
                    P::S(user_id.to_string()),
                    P::I(item_index),
                    P::S("tool_call".to_string()),
                    P::S(if failed { "failed" } else { "completed" }.to_string()),
                    P::S(payload_text.clone()),
                    P::F(tool_done),
                    P::F(tool_done),
                    P::I(seq),
                ]);
                items_written += 1;
                tool_calls_total += 1;
                if seq > plan.change_tail_cutoff {
                    change_rows.push(vec![
                        P::S(session_id.to_string()),
                        P::I(seq),
                        P::S(user_id.to_string()),
                        P::S(turn_id.to_string()),
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
                let ok_flag: i32 = if failed { 0 } else { 1 };
                let stamp = Local
                    .timestamp_millis_opt((tool_done * 1000.0) as i64)
                    .single()
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default();
                tool_rows.push(vec![
                    P::S(user_id.to_string()),
                    P::S(session_id.to_string()),
                    P::S(profile.name.to_string()),
                    P::I32(ok_flag),
                    P::S(error.clone()),
                    P::S(serde_json::to_string(&args)?),
                    P::S(serde_json::to_string(&payload["data"])?),
                    P::S(stamp),
                    P::S(serde_json::to_string(&payload)?),
                    P::F(tool_done),
                ]);
                if item_rows.len() >= BATCH_ROWS {
                    flush_batch(&tx, "thread_items", "session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq", 13, &mut item_rows).await?;
                }
                if change_rows.len() >= BATCH_ROWS {
                    flush_batch(&tx, "thread_log_changes", "session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time", 9, &mut change_rows).await?;
                }
                if tool_rows.len() >= BATCH_ROWS {
                    flush_batch(&tx, "tool_logs", "user_id,session_id,tool,ok,error,args,data,timestamp,payload,created_time", 10, &mut tool_rows).await?;
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
            let item_id = format!("{turn_id}:text-{model_round}");
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
                "item_id": item_id,
                "kind": "assistant_message",
                "status": "completed",
                "user_round": turn_index,
                "model_round": model_round,
                "turn_id": turn_id,
            }))?;
            item_index += 1;
            seq += 1;
            item_rows.push(vec![
                P::S(session_id.to_string()),
                P::S(item_id.clone()),
                P::S(turn_id.clone()),
                P::S(turn_id.clone()),
                P::S("user".to_string()),
                P::S(user_id.to_string()),
                P::I(item_index),
                P::S("assistant_message".to_string()),
                P::S("completed".to_string()),
                P::S(payload.clone()),
                P::F(done_at),
                P::F(done_at),
                P::I(seq),
            ]);
            items_written += 1;
            if seq > plan.change_tail_cutoff {
                change_rows.push(vec![
                    P::S(session_id.to_string()),
                    P::I(seq),
                    P::S(user_id.to_string()),
                    P::S(turn_id.to_string()),
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
            if item_rows.len() >= BATCH_ROWS {
                flush_batch(&tx, "thread_items", "session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq", 13, &mut item_rows).await?;
            }
            if change_rows.len() >= BATCH_ROWS {
                flush_batch(&tx, "thread_log_changes", "session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time", 9, &mut change_rows).await?;
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
                P::S(turn_id.to_string()),
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
        tx.execute(
            "UPDATE thread_turns SET updated_time=$1 WHERE session_id=$2 AND turn_id=$3",
            &[&turn_end, &session_id, &turn_id],
        )
        .await?;
        // Flush whatever is left inside this turn's transaction.
        flush_batch(&tx, "thread_items", "session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq", 13, &mut item_rows).await?;
        flush_batch(&tx, "thread_log_changes", "session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time", 9, &mut change_rows).await?;
        flush_batch(&tx, "tool_logs", "user_id,session_id,tool,ok,error,args,data,timestamp,payload,created_time", 10, &mut tool_rows).await?;
        tx.commit()
            .await
            .map_err(|err| anyhow!("commit turn transaction: {err}"))?;
        progress(turn_index, items_written);
    }

    // Final envelope: session registration, log header, metrics. One short
    // transaction; chat_sessions appears last so the list only ever shows a
    // complete thread.
    let end_time = clock;
    let tx = client
        .transaction()
        .await
        .map_err(|err| anyhow!("begin envelope transaction: {err}"))?;
    tx.execute(
        "INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES($1,$2,$3,$4,$5,$6)",
        &[&session_id, &user_id, &spec.user_rounds, &seq, &plan.start_time, &end_time],
    )
    .await?;
    tx.execute(
        "INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) VALUES($1,$2,'user_turn_total',$3,$4) \
         ON CONFLICT(session_id,metric_key) DO UPDATE SET metric_value=excluded.metric_value,updated_time=excluded.updated_time",
        &[&session_id, &user_id, &spec.user_rounds, &end_time],
    )
    .await?;
    tx.execute(
        "INSERT INTO chat_sessions(session_id,user_id,title,status,created_at,updated_at,last_message_at) VALUES($1,$2,$3,'active',$4,$5,$5)",
        &[&session_id, &user_id, &spec.title, &plan.start_time, &end_time],
    )
    .await?;
    tx.execute(
        "DELETE FROM thread_log_changes WHERE session_id=$1 AND change_seq<=$2",
        &[&session_id, &plan.change_tail_cutoff],
    )
    .await?;
    tx.commit()
        .await
        .map_err(|err| anyhow!("commit envelope transaction: {err}"))?;

    Ok(StressThreadStats {
        session_id: spec.session_id.clone(),
        user_turns: spec.user_rounds,
        items: items_written,
        tool_calls: tool_calls_total,
    })
}
