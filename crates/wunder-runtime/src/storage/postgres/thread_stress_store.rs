//! Service stress fixtures use the shared workload sampler and real writers.
use super::PostgresThreadStore;
use crate::storage::stress_model::*;
use anyhow::{ensure, Result};
use chrono::Utc;
use serde_json::{json, Value};
use wunder_core::thread_store::*;

const BATCH_RECORDS: usize = 128;
const BATCH_BYTES: usize = 2 * 1024 * 1024;

#[cfg(test)]
#[path = "thread_reader_contract_tests.rs"]
mod reader_contract;

struct Batch {
    records: Vec<CanonicalRecord>,
    bytes: usize,
    total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{postgres::PostgresStorage, ThreadLogStore};
    use wunder_core::storage_backend::LogStatsStore;

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_stress_statistics_survive_production_projection_and_user_deletion() {
        let storage = std::sync::Arc::new(
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap(),
        );
        let owner = format!("sample-owner-{}", uuid::Uuid::new_v4().simple());
        let spec = StressThreadSpec {
            session_id: format!("sample-thread-{}", uuid::Uuid::new_v4().simple()),
            title: "Sample pressure history".into(),
            workspace_id: None,
            user_rounds: 2,
            model_rounds_per_turn: 4,
            seed: Some(7),
            scenario: StressThreadScenario::Complex,
        };
        let stats = storage
            .generate_stress_thread(&owner, &spec, |_, _| {})
            .unwrap();
        let mut conn = storage.conn().unwrap();
        let rows = conn.query("SELECT record_json FROM service_thread_records WHERE thread_id=$1 AND record_type='usage' ORDER BY ordinal", &[&spec.session_id]).unwrap();
        assert_eq!(rows.len() as i64, stats.model_requests);
        let mut total_input = 0_u64;
        for row in rows {
            let record: wunder_core::thread_store::RolloutRecord =
                serde_json::from_str(row.get::<_, String>(0).as_str()).unwrap();
            let CanonicalRecord::Usage(usage) = record.record else {
                panic!("usage record");
            };
            total_input += usage.input_tokens;
            assert_eq!(usage.metadata["thread_usage"]["input_tokens"], total_input);
            assert_eq!(
                usage.metadata["request_token_breakdown"]
                    .as_object()
                    .unwrap()
                    .len(),
                14
            );
        }
        let page = storage
            .thread_view_page(&owner, &spec.session_id, "chat", None, 10)
            .unwrap();
        assert_eq!(page.chat.len(), 2);
        for turn in &page.chat {
            let message_stats = turn.assistant.stats.as_ref().unwrap();
            assert_eq!(message_stats["model_request_count"], 4);
            assert_eq!(message_stats["account_credits_consumed"], 4);
            assert!(message_stats["decode_duration_total_s"].as_f64().unwrap() > 0.0);
            assert!(message_stats["context_occupancy_tokens"].as_u64().unwrap() > 0);
        }
        let trajectory = storage
            .thread_view_trajectory_page(
                &owner,
                &spec.session_id,
                None,
                Some(&page.chat[0].root_turn_id),
                100,
            )
            .unwrap();
        assert_eq!(
            trajectory
                .trajectory
                .iter()
                .filter(|r| r.kind == "assistant_message" && r.request_token_breakdown.is_some())
                .count(),
            4
        );
        // Include admissions which have not reached thread initialization,
        // and cross the cleanup batch boundary without materializing history.
        let admissions = storage.service_threads().unwrap().admission_store();
        let mut pending_threads = Vec::new();
        for index in 0..105 {
            let id = format!("sample-pending-{index}-{}", uuid::Uuid::new_v4().simple());
            admissions
                .accept(&owner, &id, &json!({"content":"sample queued input"}))
                .unwrap();
            pending_threads.push(id);
        }
        let retained_owner = format!("sample-retained-{}", uuid::Uuid::new_v4().simple());
        let retained_thread = format!("sample-retained-{}", uuid::Uuid::new_v4().simple());
        admissions
            .accept(
                &retained_owner,
                &retained_thread,
                &json!({"content":"sample retained input"}),
            )
            .unwrap();
        assert_eq!(storage.delete_thread_logs_by_user(&owner).unwrap(), 106);
        assert_eq!(storage.delete_thread_logs_by_user(&owner).unwrap(), 0);
        assert_eq!(
            conn.query_one(
                "SELECT count(*) FROM service_thread_owners WHERE user_id=$1",
                &[&owner]
            )
            .unwrap()
            .get::<_, i64>(0),
            0
        );
        assert_eq!(
            conn.query_one(
                "SELECT count(*) FROM service_thread_admissions WHERE thread_id=ANY($1)",
                &[&pending_threads]
            )
            .unwrap()
            .get::<_, i64>(0),
            0
        );
        assert_eq!(
            conn.query_one(
                "SELECT count(*) FROM service_thread_owners WHERE thread_id=$1",
                &[&retained_thread]
            )
            .unwrap()
            .get::<_, i64>(0),
            1
        );
        assert_eq!(
            storage.delete_thread_logs_by_user(&retained_owner).unwrap(),
            1
        );
        for table in [
            "service_threads",
            "service_thread_records",
            "service_thread_sessions",
            "service_thread_items",
            "service_thread_chat_heads",
            "service_thread_display_state",
            "service_thread_admissions",
            "service_thread_owners",
        ] {
            assert_eq!(
                conn.query_one(
                    &format!("SELECT count(*) FROM {table} WHERE thread_id=$1"),
                    &[&spec.session_id]
                )
                .unwrap()
                .get::<_, i64>(0),
                0,
                "{table} retained deleted history"
            );
        }
    }
}

impl Batch {
    fn new() -> Self {
        Self {
            records: Vec::with_capacity(BATCH_RECORDS),
            bytes: 0,
            total: 0,
        }
    }

    async fn flush(&mut self, store: &PostgresThreadStore, thread: &ThreadId) -> Result<()> {
        if !self.records.is_empty() {
            store
                .append_canonical(
                    thread,
                    std::mem::replace(&mut self.records, Vec::with_capacity(BATCH_RECORDS)),
                )
                .await?;
            self.bytes = 0;
            tokio::task::yield_now().await;
        }
        Ok(())
    }

    async fn push(
        &mut self,
        store: &PostgresThreadStore,
        thread: &ThreadId,
        record: CanonicalRecord,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(&record)?.len() + 128;
        ensure!(
            bytes <= crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES,
            "stress record exceeds size limit"
        );
        if self.records.len() >= BATCH_RECORDS || self.bytes.saturating_add(bytes) > BATCH_BYTES {
            self.flush(store, thread).await?;
        }
        self.records.push(record);
        self.bytes += bytes;
        self.total += 1;
        Ok(())
    }
}

fn item(
    turn: &TurnId,
    root: &TurnId,
    suffix: &str,
    kind: CanonicalItemKind,
    payload: Value,
    now: i64,
) -> Result<CanonicalRecord> {
    Ok(CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
        CanonicalItem {
            item_id: ItemId::new(format!("{turn}:{suffix}"))?,
            turn_id: turn.clone(),
            root_turn_id: Some(root.clone()),
            created_at_ms: now,
            kind,
            payload,
            display_output: None,
            display_output_truncated: false,
        },
    )))
}

fn response(
    turn: &TurnId,
    root: &TurnId,
    suffix: &str,
    kind: CanonicalItemKind,
    payload: Value,
    now: i64,
) -> Result<CanonicalRecord> {
    Ok(CanonicalRecord::ResponseItem(ResponseItem {
        item_id: ItemId::new(format!("{turn}:{suffix}"))?,
        turn_id: turn.clone(),
        root_turn_id: Some(root.clone()),
        created_at_ms: now,
        kind,
        payload,
    }))
}

impl PostgresThreadStore {
    pub(crate) async fn generate_stress_thread(
        &self,
        user: &str,
        spec: &StressThreadSpec,
        mut progress: impl FnMut(i64, i64) + Send,
    ) -> Result<StressThreadStats> {
        validate_spec(spec)?;
        let thread = ThreadId::new(spec.session_id.clone())?;
        let seed = spec.seed.unwrap_or_else(base_seed);
        let now = Utc::now().timestamp_millis();
        let rollout = RolloutId::new(uuid::Uuid::new_v4().to_string())?;
        let admissions = self.admission_store();
        admissions.register_owner(user, thread.as_str())?;
        self.create_thread(CreateThreadRequest {
            metadata: ThreadMetadata {
                thread_id: thread.clone(), current_rollout_id: rollout.clone(), status: ThreadStatus::Active,
                created_at_ms: now, updated_at_ms: now, title: Some(spec.title.clone()), workspace_id: spec.workspace_id.clone(), pinned: false,
            },
            session_meta: SessionMeta {
                thread_id: thread.clone(), rollout_id: rollout, created_at_ms: now, source: "stress".into(),
                initial_system_prompt: synthetic_initial_system_prompt(seed, spec.scenario), initial_memory: Some(synthetic_initial_memory(seed, spec.scenario)),
                parent_thread_id: None, history_base: None,
                capabilities: json!({"synthetic":true,"profile":SYNTHETIC_PROFILE_ID,"scenario":spec.scenario.as_str(),"seed":seed,"initial_context_tokens":synthetic_initial_context_tokens(seed, spec.scenario),"writer":"postgres-canonical-v2"}),
            },
        }).await?;
        let mut rng = Rng::new(seed);
        let (mut items, mut tools) = (0, 0);
        let mut batch = Batch::new();
        let mut thread_usage = StressUsageAccumulator::default();
        for number in 1..=spec.user_rounds {
            let prompt = fill_template(rng.pick_str(user_prompts()), &mut rng, number);
            let input =
                json!({"content":prompt,"client_message_id":format!("sample-{seed}-{number}")});
            let accepted = admissions.accept(user, thread.as_str(), &input)?;
            let turn = TurnId::new(accepted["turn_id"].as_str().unwrap_or_default())?;
            let root = TurnId::new(accepted["root_turn_id"].as_str().unwrap_or_default())?;
            batch
                .push(
                    self,
                    &thread,
                    CanonicalRecord::TurnContext(TurnContext {
                        turn_id: turn.clone(),
                        root_turn_id: Some(root.clone()),
                        model: "stress-model".into(),
                        execution_policy: json!({"synthetic":true}),
                    }),
                )
                .await?;
            batch
                .push(
                    self,
                    &thread,
                    CanonicalRecord::Event(CanonicalEvent::TurnStarted {
                        turn_id: turn.clone(),
                        root_turn_id: Some(root.clone()),
                        started_at_ms: now,
                    }),
                )
                .await?;
            batch
                .push(
                    self,
                    &thread,
                    item(
                        &turn,
                        &root,
                        "user",
                        CanonicalItemKind::UserMessage,
                        json!({"role":"user","content":prompt}),
                        now,
                    )?,
                )
                .await?;
            batch
                .push(
                    self,
                    &thread,
                    response(
                        &turn,
                        &root,
                        "model-user",
                        CanonicalItemKind::UserMessage,
                        json!({"role":"user","content":prompt}),
                        now,
                    )?,
                )
                .await?;
            items += 1;
            let mut turn_statistics = StressTurnStatistics::default();
            for (index, plan) in iter_turn_plans_for_scenario(
                seed,
                number,
                spec.model_rounds_per_turn,
                spec.scenario,
            )
            .enumerate()
            {
                let answer = build_answer_text(&mut rng, &prompt, number);
                let reasoning =
                    build_reasoning_text(&mut rng, plan.reasoning_tokens.unwrap_or_default());
                let mut generated = Vec::with_capacity(plan.tool_profiles.len());
                let mut calls = Vec::with_capacity(plan.tool_profiles.len());
                for (tool_index, profile) in plan.tool_profiles.iter().enumerate() {
                    let call_id = format!("{turn}:call-{index}-{tool_index}");
                    let args = build_tool_args(&mut rng, profile, number);
                    let result = build_tool_result_for_scenario(
                        &mut rng,
                        profile,
                        &args,
                        (plan.tool_duration_s[tool_index] * 1000.0) as i64,
                        spec.scenario,
                    )?;
                    calls.push(json!({"id":call_id,"type":"function","function":{"name":profile.function_name,"arguments":serde_json::to_string(&args)?}}));
                    generated.push((profile, call_id, args, result));
                }
                let mut payload =
                    json!({"role":"assistant","content":answer,"reasoning_content":reasoning});
                if !calls.is_empty() {
                    payload["tool_calls"] = json!(calls);
                }
                let statistics = turn_statistics.record_model(
                    &mut thread_usage,
                    seed,
                    spec.scenario,
                    &prompt,
                    &turn,
                    &plan,
                );
                batch
                    .push(self, &thread, CanonicalRecord::Usage(statistics.usage))
                    .await?;
                batch
                    .push(
                        self,
                        &thread,
                        response(
                            &turn,
                            &root,
                            &format!("response-{index}"),
                            CanonicalItemKind::AssistantMessage,
                            payload.clone(),
                            now,
                        )?,
                    )
                    .await?;
                payload["model_round"] = json!(index + 1);
                payload["request_token_breakdown"] = statistics.request_token_breakdown;
                payload["meta"] = json!({
                    "type": if generated.is_empty() { "model_response" } else { "tool_call" },
                    "message_stats": statistics.message_stats,
                });
                batch
                    .push(
                        self,
                        &thread,
                        item(
                            &turn,
                            &root,
                            &format!("text-{index}"),
                            CanonicalItemKind::AssistantMessage,
                            payload,
                            now,
                        )?,
                    )
                    .await?;
                items += 1;
                for (tool_index, (profile, call_id, args, result)) in
                    generated.into_iter().enumerate()
                {
                    let payload = json!({"tool":profile.name,"args":args,"data":result.data,"error":result.error,"ok":!result.failed});
                    batch.push(self, &thread, response(&turn, &root, &format!("call-{index}-{tool_index}"), CanonicalItemKind::ToolResult,
                        json!({"role":"tool","tool_call_id":call_id,"content":serde_json::to_string(&payload)?}), now)?).await?;
                    let mut record = item(
                        &turn,
                        &root,
                        &format!("tool-{index}-{tool_index}"),
                        CanonicalItemKind::ToolResult,
                        payload,
                        now,
                    )?;
                    if let CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
                        ref mut completed,
                    )) = record
                    {
                        completed.display_output = Some(result.data_text);
                    }
                    batch.push(self, &thread, record).await?;
                    items += 1;
                    tools += 1;
                    turn_statistics.complete_tool(plan.tool_duration_s[tool_index]);
                }
            }
            batch
                .push(
                    self,
                    &thread,
                    CanonicalRecord::Event(CanonicalEvent::TurnCompleted {
                        turn_id: turn.clone(),
                        completed_at_ms: now,
                    }),
                )
                .await?;
            batch.flush(self, &thread).await?;
            ensure!(
                admissions.update_with_control(
                    thread.as_str(),
                    turn.as_str(),
                    "completed",
                    true,
                    &Value::Null
                )?,
                "stress admission did not complete"
            );
            progress(number, items);
        }
        Ok(StressThreadStats {
            session_id: spec.session_id.clone(),
            user_turns: spec.user_rounds,
            items,
            tool_calls: tools,
            model_requests: spec.user_rounds * spec.model_rounds_per_turn,
            model_actions: spec.user_rounds * spec.model_rounds_per_turn + tools,
            // Include the frozen SessionMeta record committed by create_thread.
            rollout_records: self
                .history_position_public(&thread)
                .await?
                .end_ordinal_exclusive,
            seed,
            profile: SYNTHETIC_PROFILE_ID,
            scenario: spec.scenario.as_str(),
            initial_context_tokens: synthetic_initial_context_tokens(seed, spec.scenario),
            storage_bytes: None,
        })
    }
}
