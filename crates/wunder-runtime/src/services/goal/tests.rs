//! Goal domain unit tests: vocabulary, execution-time authority, prompt
//! texts, the CAS service lifecycle, the `/goal` command face, and the
//! tool-output projections.

use super::authority::{
    completion_authority, has_direct_human_source, mark_human_source, read_goal_round_tag,
    require_direct_human, GoalToolAuthority,
};
use super::command::{execute_goal_command, parse_goal_command, GoalCommand};
use super::prompt::{
    render_goal_guidance, render_goal_guidance_with_threshold, render_goal_round_prompt,
    render_wrapup_context,
};
use super::service::{
    goal_payload, goal_tool_value, goal_tool_value_none, validate_objective, GoalService,
};
use super::tools::{goal_tool_specs, is_goal_tool_name, read_wrapup};
use super::types::*;
use crate::storage::{SessionGoalRecord, SqliteStorage, StorageBackend, StorageLifecycle};
use serde_json::json;
use std::sync::Arc;

const USER: &str = "user-1";
const SESSION: &str = "session-1";

fn assert_code(error: anyhow::Error, code: &str) {
    let error = error
        .downcast_ref::<GoalError>()
        .expect("expected GoalError");
    assert_eq!(error.code, code);
}

async fn setup() -> (GoalService, Arc<dyn StorageBackend>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(dir.path().join("goal.db").to_string_lossy().into());
    storage.ensure_initialized().unwrap();
    (
        GoalService::new(),
        Arc::new(storage) as Arc<dyn StorageBackend>,
        dir,
    )
}

fn view(phase: &str, revision: i64, rounds_started: i64, activation: GoalActivation) -> GoalView {
    GoalView {
        record: SessionGoalRecord {
            goal_id: "goal-1".into(),
            session_id: SESSION.into(),
            user_id: USER.into(),
            revision,
            objective: "objective".into(),
            phase: phase.to_string(),
            blocked_code: None,
            blocked_message: None,
            max_goal_rounds: 8,
            rounds_started,
            created_at: 0.0,
            updated_at: 0.0,
        },
        activation,
    }
}

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

#[test]
fn goal_phase_parse_and_render_roundtrip() {
    for phase in [
        GoalPhase::Active,
        GoalPhase::Paused,
        GoalPhase::Blocked,
        GoalPhase::Complete,
    ] {
        assert_eq!(GoalPhase::parse(phase.as_str()), Some(phase));
    }
    assert_eq!(GoalPhase::parse("ACTIVE"), Some(GoalPhase::Active));
    assert_eq!(GoalPhase::parse(" running "), None);
}

#[test]
fn blocked_reason_defaults_code_while_blocked_only() {
    let active = view(PHASE_ACTIVE, 1, 0, GoalActivation::Armed);
    assert_eq!(active.blocked_reason(), None);

    let blocked = view(PHASE_BLOCKED, 2, 1, GoalActivation::Disarmed);
    assert_eq!(
        blocked.blocked_reason(),
        Some(GoalBlockReason {
            code: "blocked".into(),
            message: String::new(),
        })
    );
}

#[test]
fn blocked_reason_carries_stored_code_and_message() {
    let mut blocked = view(PHASE_BLOCKED, 2, 1, GoalActivation::Disarmed);
    blocked.record.blocked_code = Some("queue-failed".into());
    blocked.record.blocked_message = Some("could not queue".into());
    assert_eq!(
        blocked.blocked_reason(),
        Some(GoalBlockReason {
            code: "queue-failed".into(),
            message: "could not queue".into(),
        })
    );
}

// ---------------------------------------------------------------------------
// authority
// ---------------------------------------------------------------------------

#[test]
fn mark_human_source_stamps_and_preserves_existing_keys() {
    let mut overrides = None;
    mark_human_source(&mut overrides);
    assert_eq!(overrides, Some(json!({ "__source": "human" })));

    let mut overrides = Some(json!({ "existing": 1 }));
    mark_human_source(&mut overrides);
    assert_eq!(
        overrides,
        Some(json!({ "existing": 1, "__source": "human" }))
    );
}

#[test]
fn human_source_check_is_exact_after_trim() {
    assert!(has_direct_human_source(Some(
        &json!({ "__source": "human" })
    )));
    assert!(has_direct_human_source(Some(
        &json!({ "__source": " human " })
    )));
    assert!(!has_direct_human_source(Some(
        &json!({ "__source": "model" })
    )));
    assert!(!has_direct_human_source(Some(&json!({}))));
    assert!(!has_direct_human_source(None));
}

#[test]
fn goal_round_tag_reading_rejects_invalid_values() {
    let tag = read_goal_round_tag(Some(&json!({
        "__goal_round": { "goal_id": "g", "revision": 2, "round": 3 }
    })))
    .unwrap();
    assert_eq!(
        tag,
        GoalRoundTag {
            goal_id: "g".into(),
            revision: 2,
            round: 3,
        }
    );

    for invalid in [
        json!({ "goal_id": "", "revision": 2, "round": 3 }),
        json!({ "goal_id": "  ", "revision": 2, "round": 3 }),
        json!({ "goal_id": "g", "revision": 0, "round": 3 }),
        json!({ "goal_id": "g", "revision": 2, "round": 0 }),
        json!({ "goal_id": "g", "revision": "2", "round": 3 }),
    ] {
        assert!(
            read_goal_round_tag(Some(&json!({ "__goal_round": invalid }))).is_none(),
            "invalid tag must be rejected"
        );
    }
    assert!(read_goal_round_tag(Some(&json!({}))).is_none());
    assert!(read_goal_round_tag(None).is_none());
}

#[test]
fn completion_authority_accepts_human_and_the_exact_admitted_round() {
    let goal = view(PHASE_ACTIVE, 2, 3, GoalActivation::Armed);

    let human = json!({ "__source": "human" });
    assert!(matches!(
        completion_authority(Some(&goal), Some(&human)),
        Ok(GoalToolAuthority::DirectHuman)
    ));

    let round = json!({
        "__goal_round": { "goal_id": "goal-1", "revision": 2, "round": 3 }
    });
    assert!(matches!(
        completion_authority(Some(&goal), Some(&round)),
        Ok(GoalToolAuthority::GoalRound(_))
    ));
}

#[test]
fn completion_authority_rejects_turns_without_the_exact_round() {
    let goal = view(PHASE_ACTIVE, 2, 3, GoalActivation::Armed);
    for tag in [
        json!({ "goal_id": "goal-1", "revision": 1, "round": 3 }),
        json!({ "goal_id": "goal-1", "revision": 2, "round": 2 }),
        json!({ "goal_id": "goal-9", "revision": 2, "round": 3 }),
    ] {
        let overrides = json!({ "__goal_round": tag });
        let error = completion_authority(Some(&goal), Some(&overrides)).unwrap_err();
        assert_eq!(error.code, ERR_TOOL_AUTHORITY_REQUIRED);
    }
    assert!(completion_authority(Some(&goal), None).is_err());
    assert!(completion_authority(None, None).is_err());
}

#[test]
fn require_direct_human_rejects_model_turns() {
    require_direct_human(Some(&json!({ "__source": "human" }))).unwrap();
    let error = require_direct_human(None).unwrap_err();
    assert_eq!(error.code, ERR_TOOL_AUTHORITY_REQUIRED);
}

// ---------------------------------------------------------------------------
// prompt
// ---------------------------------------------------------------------------

#[test]
fn goal_round_prompt_wraps_objective_as_json_data() {
    let prompt = render_goal_round_prompt("ship the release", 2, 8);
    assert!(prompt.starts_with("<goal_round>"));
    assert!(prompt.contains(r#"Objective: "ship the release""#));
    assert!(prompt.contains("Round: 2/8"));
    assert!(prompt.ends_with("</goal_round>"));

    // Quotes in the objective stay JSON-escaped so they cannot escalate.
    let escaped = render_goal_round_prompt("say \"hi\"", 1, 8);
    assert!(escaped.contains(r#"Objective: "say \"hi\"""#));
}

#[test]
fn guidance_states_the_blocked_threshold() {
    assert!(render_goal_guidance().contains("at least 3 consecutive"));
    assert!(render_goal_guidance_with_threshold(5).contains("at least 5 consecutive"));
}

#[test]
fn wrapup_context_renders_complete_and_blocked_forms() {
    let complete = render_wrapup_context("ship it", None);
    assert!(complete.starts_with("<goal_complete>"));
    assert!(complete.contains(r#"Objective: "ship it""#));
    assert!(complete.ends_with("</goal_complete>"));

    let blocked = render_wrapup_context("ship it", Some("waiting on review"));
    assert!(blocked.starts_with("<goal_blocked>"));
    assert!(blocked.contains(r#"Objective: "ship it""#));
    assert!(blocked.contains(r#"Blocked: "waiting on review""#));
    assert!(blocked.ends_with("</goal_blocked>"));
}

// ---------------------------------------------------------------------------
// service (SqliteStorage)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_produces_active_armed_goal_with_defaults() {
    let (service, storage, _dir) = setup().await;
    assert!(service
        .get_view(&storage, USER, SESSION)
        .await
        .unwrap()
        .is_none());

    let created = service
        .create(storage.clone(), USER, SESSION, "  ship it  ", None)
        .await
        .unwrap();
    assert_eq!(created.record.objective, "ship it");
    assert_eq!(created.record.phase, PHASE_ACTIVE);
    assert_eq!(created.record.revision, 1);
    assert_eq!(created.record.rounds_started, 0);
    assert_eq!(created.record.max_goal_rounds, DEFAULT_MAX_GOAL_ROUNDS);
    assert_eq!(created.activation, GoalActivation::Armed);

    let loaded = service
        .get_view(&storage, USER, SESSION)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.record.goal_id, created.record.goal_id);
    assert_eq!(loaded.activation, GoalActivation::Armed);
}

#[tokio::test]
async fn create_honors_custom_cap_and_rejects_invalid_ranges() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "ship it", Some(8))
        .await
        .unwrap();
    assert_eq!(created.record.max_goal_rounds, 8);

    assert_code(
        service
            .create(storage.clone(), USER, SESSION, "ship it", Some(0))
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_MAX_ROUNDS,
    );
    assert_code(
        service
            .create(storage, USER, SESSION, "ship it", Some(10_001))
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_MAX_ROUNDS,
    );
}

#[tokio::test]
async fn create_rejects_existing_goal_until_complete() {
    let (service, storage, _dir) = setup().await;
    let first = service
        .create(storage.clone(), USER, SESSION, "first", None)
        .await
        .unwrap();
    assert_code(
        service
            .create(storage.clone(), USER, SESSION, "second", None)
            .await
            .unwrap_err(),
        ERR_GOAL_ALREADY_EXISTS,
    );

    service
        .complete(
            storage.clone(),
            USER,
            SESSION,
            &GoalRef {
                goal_id: first.record.goal_id.clone(),
                revision: first.record.revision,
            },
        )
        .await
        .unwrap();
    let second = service
        .create(storage, USER, SESSION, "second", None)
        .await
        .unwrap();
    assert_ne!(second.record.goal_id, first.record.goal_id);
    assert_eq!(second.record.phase, PHASE_ACTIVE);
}

#[tokio::test]
async fn edit_bumps_revision_and_enforces_cas() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "first", None)
        .await
        .unwrap();
    let revision_1 = GoalRef {
        goal_id: created.record.goal_id.clone(),
        revision: 1,
    };

    let edited = service
        .edit(
            storage.clone(),
            USER,
            SESSION,
            &revision_1,
            "second",
            Some(9),
        )
        .await
        .unwrap();
    assert_eq!(edited.record.revision, 2);
    assert_eq!(edited.record.objective, "second");
    assert_eq!(edited.record.max_goal_rounds, 9);

    // The old revision is now stale.
    assert_code(
        service
            .edit(storage.clone(), USER, SESSION, &revision_1, "third", None)
            .await
            .unwrap_err(),
        ERR_GOAL_STALE_REVISION,
    );
    assert_code(
        service
            .edit(
                storage,
                USER,
                SESSION,
                &GoalRef {
                    goal_id: "other".into(),
                    revision: 2,
                },
                "third",
                None,
            )
            .await
            .unwrap_err(),
        ERR_GOAL_NOT_FOUND,
    );
}

#[tokio::test]
async fn edit_rejects_cap_below_started_rounds() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "ship", None)
        .await
        .unwrap();
    let tag = GoalRoundTag {
        goal_id: created.record.goal_id.clone(),
        revision: created.record.revision,
        round: 3,
    };
    assert!(service
        .admit_round(&storage, USER, SESSION, &tag)
        .await
        .unwrap());

    assert_code(
        service
            .edit(
                storage.clone(),
                USER,
                SESSION,
                &GoalRef {
                    goal_id: tag.goal_id.clone(),
                    revision: tag.revision,
                },
                "ship",
                Some(2),
            )
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_MAX_ROUNDS,
    );
    let edited = service
        .edit(
            storage,
            USER,
            SESSION,
            &GoalRef {
                goal_id: tag.goal_id,
                revision: tag.revision,
            },
            "ship",
            Some(3),
        )
        .await
        .unwrap();
    assert_eq!(edited.record.max_goal_rounds, 3);
}

#[tokio::test]
async fn pause_resume_and_block_lifecycle() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "ship", None)
        .await
        .unwrap();
    let mut reference = GoalRef {
        goal_id: created.record.goal_id.clone(),
        revision: created.record.revision,
    };

    let paused = service
        .pause(storage.clone(), USER, SESSION, &reference)
        .await
        .unwrap();
    assert_eq!(paused.record.phase, PHASE_PAUSED);
    assert_eq!(paused.record.revision, 2);
    assert_eq!(paused.activation, GoalActivation::Disarmed);
    reference.revision = paused.record.revision;
    assert_code(
        service
            .pause(storage.clone(), USER, SESSION, &reference)
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_TRANSITION,
    );

    // Only an active goal can be blocked.
    assert_code(
        service
            .block(
                storage.clone(),
                USER,
                SESSION,
                &reference,
                GoalBlockReason {
                    code: "queue-failed".into(),
                    message: "no queue".into(),
                },
            )
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_TRANSITION,
    );

    let resumed = service
        .resume(storage.clone(), USER, SESSION, &reference)
        .await
        .unwrap();
    assert_eq!(resumed.record.phase, PHASE_ACTIVE);
    assert_eq!(resumed.record.blocked_code, None);
    assert_eq!(resumed.activation, GoalActivation::Armed);
    reference.revision = resumed.record.revision;
    assert_code(
        service
            .resume(storage.clone(), USER, SESSION, &reference)
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_TRANSITION,
    );

    let blocked = service
        .block(
            storage.clone(),
            USER,
            SESSION,
            &reference,
            GoalBlockReason {
                code: "queue-failed".into(),
                message: "no queue".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(blocked.record.phase, PHASE_BLOCKED);
    assert_eq!(blocked.record.blocked_code.as_deref(), Some("queue-failed"));
    assert_eq!(blocked.record.blocked_message.as_deref(), Some("no queue"));
    assert_eq!(blocked.activation, GoalActivation::Disarmed);

    reference.revision = blocked.record.revision;
    let unblocked = service
        .resume(storage, USER, SESSION, &reference)
        .await
        .unwrap();
    assert_eq!(unblocked.record.phase, PHASE_ACTIVE);
    assert_eq!(unblocked.activation, GoalActivation::Armed);
}

#[tokio::test]
async fn complete_is_terminal() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "ship", None)
        .await
        .unwrap();
    let reference = GoalRef {
        goal_id: created.record.goal_id.clone(),
        revision: created.record.revision,
    };

    let completed = service
        .complete(storage.clone(), USER, SESSION, &reference)
        .await
        .unwrap();
    assert_eq!(completed.record.phase, PHASE_COMPLETE);
    assert_eq!(completed.record.revision, 2);
    assert_eq!(completed.activation, GoalActivation::Disarmed);
    let reference = GoalRef {
        goal_id: completed.record.goal_id.clone(),
        revision: completed.record.revision,
    };

    assert_code(
        service
            .complete(storage.clone(), USER, SESSION, &reference)
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_TRANSITION,
    );
    assert_code(
        service
            .pause(storage.clone(), USER, SESSION, &reference)
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_TRANSITION,
    );
    assert_code(
        service
            .block(
                storage,
                USER,
                SESSION,
                &reference,
                GoalBlockReason {
                    code: "x".into(),
                    message: "y".into(),
                },
            )
            .await
            .unwrap_err(),
        ERR_GOAL_INVALID_TRANSITION,
    );
}

#[tokio::test]
async fn clear_removes_goal_and_disarms() {
    let (service, storage, _dir) = setup().await;
    service
        .create(storage.clone(), USER, SESSION, "ship", None)
        .await
        .unwrap();

    service.clear(storage.clone(), USER, SESSION).await.unwrap();
    assert!(service
        .get_view(&storage, USER, SESSION)
        .await
        .unwrap()
        .is_none());
    assert_eq!(service.activation(SESSION), GoalActivation::Disarmed);

    // Clearing an absent goal is a no-op.
    service.clear(storage, USER, SESSION).await.unwrap();
}

#[tokio::test]
async fn list_views_projects_sessions_in_one_query() {
    let (service, storage, _dir) = setup().await;
    service
        .create(storage.clone(), USER, "session-a", "alpha", None)
        .await
        .unwrap();
    service
        .create(storage.clone(), USER, "session-b", "beta", None)
        .await
        .unwrap();

    let views = service
        .list_views(
            &storage,
            USER,
            &["session-a".into(), "session-b".into(), "   ".into()],
        )
        .await
        .unwrap();
    let mut objectives = views
        .iter()
        .map(|view| view.record.objective.clone())
        .collect::<Vec<_>>();
    objectives.sort();
    assert_eq!(objectives, ["alpha", "beta"]);
}

#[tokio::test]
async fn admit_round_folds_rounds_without_bumping_revision() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "ship", Some(8))
        .await
        .unwrap();
    let tag = GoalRoundTag {
        goal_id: created.record.goal_id.clone(),
        revision: created.record.revision,
        round: 1,
    };

    assert!(service
        .admit_round(&storage, USER, SESSION, &tag)
        .await
        .unwrap());
    let view = service
        .get_view(&storage, USER, SESSION)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(view.record.rounds_started, 1);
    assert_eq!(view.record.revision, 1);

    // Re-admitting a started round is idempotent.
    assert!(service
        .admit_round(&storage, USER, SESSION, &tag)
        .await
        .unwrap());

    // Wrong goal id or revision never admits.
    assert!(!service
        .admit_round(
            &storage,
            USER,
            SESSION,
            &GoalRoundTag {
                goal_id: "other".into(),
                revision: 1,
                round: 2,
            }
        )
        .await
        .unwrap());
    assert!(!service
        .admit_round(
            &storage,
            USER,
            SESSION,
            &GoalRoundTag {
                goal_id: tag.goal_id.clone(),
                revision: 9,
                round: 2,
            }
        )
        .await
        .unwrap());
}

#[test]
fn objective_validation_trims_and_bounds_length() {
    assert_eq!(validate_objective("  ship it ").unwrap(), "ship it");
    assert_code(
        validate_objective("   ").unwrap_err(),
        ERR_GOAL_INVALID_OBJECTIVE,
    );
    let max = "x".repeat(MAX_OBJECTIVE_CHARS);
    assert!(validate_objective(&max).is_ok());
    assert_code(
        validate_objective(format!("{max}x")).unwrap_err(),
        ERR_GOAL_INVALID_OBJECTIVE,
    );
}

#[test]
fn payload_and_tool_projections_have_stable_shapes() {
    let mut goal = view(PHASE_BLOCKED, 2, 1, GoalActivation::Armed);
    goal.record.blocked_code = Some("queue-failed".into());
    goal.record.blocked_message = Some("no queue".into());

    let payload = goal_payload(&goal);
    assert_eq!(payload["goal_id"], "goal-1");
    assert_eq!(payload["revision"], 2);
    assert_eq!(payload["phase"], "blocked");
    assert_eq!(payload["rounds_started"], 1);
    assert_eq!(payload["activation"], "armed");

    let tool_value = goal_tool_value(&goal);
    assert_eq!(tool_value["goal"]["id"], "goal-1");
    assert_eq!(tool_value["goal"]["revision"], 2);
    assert_eq!(tool_value["goal"]["roundsStarted"], 1);
    assert_eq!(tool_value["goal"]["maxGoalRounds"], 8);
    assert_eq!(tool_value["goal"]["blockedReason"]["code"], "queue-failed");
    assert_eq!(tool_value["activation"], "armed");

    // Only blocked goals carry blockedReason.
    let active = view(PHASE_ACTIVE, 1, 0, GoalActivation::Armed);
    assert!(goal_tool_value(&active)["goal"]
        .get("blockedReason")
        .is_none());
    assert_eq!(goal_tool_value_none(), json!({ "goal": null }));
}

// ---------------------------------------------------------------------------
// command face
// ---------------------------------------------------------------------------

#[test]
fn parse_goal_command_grammar() {
    assert_eq!(parse_goal_command(""), GoalCommand::Show);
    assert_eq!(parse_goal_command("   "), GoalCommand::Show);
    assert_eq!(parse_goal_command("clear"), GoalCommand::Clear);
    assert_eq!(parse_goal_command(" PAUSE "), GoalCommand::Pause);
    assert_eq!(parse_goal_command("Resume"), GoalCommand::Resume);
    assert_eq!(parse_goal_command("edit"), GoalCommand::InvalidEdit);
    assert_eq!(
        parse_goal_command("EDIT new objective"),
        GoalCommand::Edit {
            objective: "new objective".into()
        }
    );
    assert_eq!(
        parse_goal_command("editor"),
        GoalCommand::Create {
            objective: "editor".into()
        }
    );
    assert_eq!(
        parse_goal_command("ship the release"),
        GoalCommand::Create {
            objective: "ship the release".into()
        }
    );
}

#[tokio::test]
async fn execute_goal_command_renders_and_guards_missing_goal() {
    let (service, storage, _dir) = setup().await;

    let (reply, payload) =
        execute_goal_command(&service, storage.clone(), USER, SESSION, GoalCommand::Show)
            .await
            .unwrap();
    assert!(reply.contains("No goal is currently set"));
    assert!(payload.is_none());

    let (reply, payload) =
        execute_goal_command(&service, storage.clone(), USER, SESSION, GoalCommand::Pause)
            .await
            .unwrap();
    assert!(reply.contains("requires one"));
    assert!(payload.is_none());

    let (reply, _) = execute_goal_command(
        &service,
        storage.clone(),
        USER,
        SESSION,
        GoalCommand::InvalidEdit,
    )
    .await
    .unwrap();
    assert!(reply.contains("replacement objective"));

    let (reply, _) = execute_goal_command(&service, storage, USER, SESSION, GoalCommand::Clear)
        .await
        .unwrap();
    assert!(reply.contains("No goal to clear."));
}

#[tokio::test]
async fn execute_goal_command_show_renders_current_state() {
    let (service, storage, _dir) = setup().await;
    service
        .create(storage.clone(), USER, SESSION, "ship it", Some(8))
        .await
        .unwrap();

    let (reply, payload) =
        execute_goal_command(&service, storage, USER, SESSION, GoalCommand::Show)
            .await
            .unwrap();
    assert!(reply.contains("Status: active"));
    assert!(reply.contains("Objective: ship it"));
    assert!(reply.contains("Rounds: 0/8"));
    assert!(reply.contains("Activation: armed"));
    assert!(payload.is_some());
}

#[tokio::test]
async fn execute_goal_command_full_lifecycle() {
    let (service, storage, _dir) = setup().await;

    let (reply, payload) = execute_goal_command(
        &service,
        storage.clone(),
        USER,
        SESSION,
        GoalCommand::Create {
            objective: "ship it".into(),
        },
    )
    .await
    .unwrap();
    assert!(reply.contains("Goal created"));
    assert_eq!(payload.unwrap()["phase"], "active");

    let (reply, payload) = execute_goal_command(
        &service,
        storage.clone(),
        USER,
        SESSION,
        GoalCommand::Create {
            objective: "another".into(),
        },
    )
    .await
    .unwrap();
    assert!(reply.contains("already"));
    assert!(payload.is_some());

    let (reply, payload) = execute_goal_command(
        &service,
        storage.clone(),
        USER,
        SESSION,
        GoalCommand::Edit {
            objective: "revised".into(),
        },
    )
    .await
    .unwrap();
    assert!(reply.contains("Goal updated"));
    assert_eq!(payload.unwrap()["objective"], "revised");

    let (reply, payload) =
        execute_goal_command(&service, storage.clone(), USER, SESSION, GoalCommand::Pause)
            .await
            .unwrap();
    assert!(reply.contains("Goal paused"));
    assert_eq!(payload.unwrap()["phase"], "paused");

    let (reply, payload) = execute_goal_command(
        &service,
        storage.clone(),
        USER,
        SESSION,
        GoalCommand::Resume,
    )
    .await
    .unwrap();
    assert!(reply.contains("Goal resumed"));
    assert_eq!(payload.unwrap()["phase"], "active");

    let (reply, _) =
        execute_goal_command(&service, storage.clone(), USER, SESSION, GoalCommand::Clear)
            .await
            .unwrap();
    assert!(reply.contains("Goal cleared"));
    assert!(service
        .get_view(&storage, USER, SESSION)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn execute_goal_command_edit_replaces_completed_goal() {
    let (service, storage, _dir) = setup().await;
    let created = service
        .create(storage.clone(), USER, SESSION, "first", None)
        .await
        .unwrap();
    service
        .complete(
            storage.clone(),
            USER,
            SESSION,
            &GoalRef {
                goal_id: created.record.goal_id.clone(),
                revision: created.record.revision,
            },
        )
        .await
        .unwrap();

    let (reply, payload) = execute_goal_command(
        &service,
        storage,
        USER,
        SESSION,
        GoalCommand::Edit {
            objective: "next".into(),
        },
    )
    .await
    .unwrap();
    assert!(reply.contains("Goal created"));
    let payload = payload.unwrap();
    assert_eq!(payload["objective"], "next");
    assert_eq!(payload["phase"], "active");
    assert_ne!(payload["goal_id"], created.record.goal_id);
}

// ---------------------------------------------------------------------------
// tool surface (pure helpers)
// ---------------------------------------------------------------------------

#[test]
fn goal_tool_specs_cover_three_tools() {
    let specs = goal_tool_specs();
    let names = specs
        .iter()
        .map(|spec| spec.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["get_goal", "create_goal", "update_goal"]);
    assert_eq!(specs[1].input_schema["required"], json!(["objective"]));
    assert_eq!(
        specs[2].input_schema["required"],
        json!(["goal_id", "revision", "action"])
    );
}

#[test]
fn goal_tool_name_matching_trims() {
    assert!(is_goal_tool_name("get_goal"));
    assert!(is_goal_tool_name(" get_goal "));
    assert!(is_goal_tool_name("update_goal"));
    assert!(!is_goal_tool_name("get_goalx"));
    assert!(!is_goal_tool_name(""));
}

#[test]
fn read_wrapup_extracts_text() {
    let result = json!({ "__wrapup": { "kind": "complete", "text": "closing instructions" } });
    assert_eq!(
        read_wrapup(&result).as_deref(),
        Some("closing instructions")
    );
    assert!(read_wrapup(&json!({ "goal": null })).is_none());
}
