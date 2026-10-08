//! Human-facing `/goal` command over the persisted same-session goal domain.

use super::service::{goal_payload, GoalService};
use super::types::*;
use crate::storage::StorageBackend;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

pub const USAGE: &str = "Usage: /goal [<objective>|clear|edit <objective>|pause|resume]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalCommand {
    Show,
    Create { objective: String },
    Edit { objective: String },
    InvalidEdit,
    Pause,
    Resume,
    Clear,
}

/// Parse only the grammar owned by `/goal`; arbitrary other input is an
/// objective.
pub fn parse_goal_command(raw_input: &str) -> GoalCommand {
    let input = raw_input.trim();
    if input.is_empty() {
        return GoalCommand::Show;
    }
    let control = input.to_ascii_lowercase();
    match control.as_str() {
        "clear" => return GoalCommand::Clear,
        "pause" => return GoalCommand::Pause,
        "resume" => return GoalCommand::Resume,
        "edit" => return GoalCommand::InvalidEdit,
        _ => {}
    }
    if input.len() >= 4 && input[..4].eq_ignore_ascii_case("edit") {
        let rest = &input[4..];
        if rest.starts_with(' ') || rest.starts_with('\t') {
            return GoalCommand::Edit {
                objective: rest.trim().to_string(),
            };
        }
    }
    GoalCommand::Create {
        objective: input.to_string(),
    }
}

fn phase_label(phase: GoalPhase) -> &'static str {
    phase.as_str()
}

/// Commands that are meaningful from one exact live state.
fn command_hint(goal: &GoalView) -> String {
    if goal.phase() == GoalPhase::Active {
        return if goal.activation == GoalActivation::Armed {
            "/goal edit <objective>, /goal pause, /goal clear".to_string()
        } else {
            "/goal edit <objective>, /goal resume, /goal clear".to_string()
        };
    }
    match goal.phase() {
        GoalPhase::Paused | GoalPhase::Blocked => {
            "/goal edit <objective>, /goal resume, /goal clear".to_string()
        }
        GoalPhase::Complete => "/goal <objective>, /goal clear".to_string(),
        GoalPhase::Active => unreachable!(),
    }
}

/// Render direct UI output without exposing compare-and-set internals.
pub fn render_goal(title: &str, goal: &GoalView) -> String {
    let blocker = goal
        .blocked_reason()
        .map(|reason| format!("Blocker: {}: {}", reason.code, reason.message))
        .into_iter()
        .collect::<Vec<_>>();
    format!(
        "{}\nStatus: {}\n{}\nObjective: {}\nRounds: {}/{}\nActivation: {}\n\nCommands: {}",
        title,
        phase_label(goal.phase()),
        blocker.join("\n"),
        goal.record.objective,
        goal.record.rounds_started,
        goal.record.max_goal_rounds,
        goal.activation.as_str(),
        command_hint(goal),
    )
}

fn missing_goal(action: &str) -> String {
    format!("No goal is currently set; /goal {action} requires one. {USAGE}")
}

/// Execute one parsed human command through the domain that owns persistence.
/// Returns the rendered reply plus the goal projection after the change.
pub async fn execute_goal_command(
    service: &GoalService,
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
    command: GoalCommand,
) -> Result<(String, Option<Value>)> {
    let current = service
        .get_view(&storage, user_id, session_id)
        .await
        .ok()
        .flatten();
    // Error paths below may run after `current` was consumed by a branch;
    // snapshot the projection up front.
    let current_payload = current.as_ref().map(goal_payload);
    let result: Result<(String, Option<Value>)> = match command {
        GoalCommand::Show => Ok(match current {
            None => (format!("No goal is currently set.\n{USAGE}"), None),
            Some(view) => (render_goal("Goal", &view), Some(goal_payload(&view))),
        }),
        GoalCommand::InvalidEdit => Ok((
            format!("Goal editing requires a replacement objective.\n{USAGE}"),
            current.as_ref().map(goal_payload),
        )),
        GoalCommand::Create { objective } => {
            if let Some(view) = current.as_ref() {
                if view.phase() != GoalPhase::Complete {
                    return Ok((
                        format!(
                            "A goal is already {}. Use /goal edit <objective> to change it or /goal clear before replacing it.",
                            phase_label(view.phase())
                        ),
                        Some(goal_payload(view)),
                    ));
                }
            }
            let created = service
                .create(storage, user_id, session_id, &objective, None)
                .await?;
            Ok((
                render_goal("Goal created", &created),
                Some(goal_payload(&created)),
            ))
        }
        GoalCommand::Edit { objective } => {
            let Some(view) = current else {
                return Ok((missing_goal("edit"), None));
            };
            if view.phase() == GoalPhase::Complete {
                let replaced = service
                    .create(storage, user_id, session_id, &objective, None)
                    .await?;
                return Ok((
                    render_goal("Goal created", &replaced),
                    Some(goal_payload(&replaced)),
                ));
            }
            let edited = service
                .edit(
                    storage,
                    user_id,
                    session_id,
                    &GoalRef {
                        goal_id: view.record.goal_id.clone(),
                        revision: view.record.revision,
                    },
                    &objective,
                    None,
                )
                .await?;
            Ok((
                render_goal("Goal updated", &edited),
                Some(goal_payload(&edited)),
            ))
        }
        GoalCommand::Pause => {
            let Some(view) = current else {
                return Ok((missing_goal("pause"), None));
            };
            let paused = service
                .pause(
                    storage,
                    user_id,
                    session_id,
                    &GoalRef {
                        goal_id: view.record.goal_id.clone(),
                        revision: view.record.revision,
                    },
                )
                .await?;
            Ok((
                render_goal("Goal paused", &paused),
                Some(goal_payload(&paused)),
            ))
        }
        GoalCommand::Resume => {
            let Some(view) = current else {
                return Ok((missing_goal("resume"), None));
            };
            let resumed = service
                .resume(
                    storage,
                    user_id,
                    session_id,
                    &GoalRef {
                        goal_id: view.record.goal_id.clone(),
                        revision: view.record.revision,
                    },
                )
                .await?;
            Ok((
                render_goal("Goal resumed", &resumed),
                Some(goal_payload(&resumed)),
            ))
        }
        GoalCommand::Clear => match current {
            None => Ok(("No goal to clear.".to_string(), None)),
            Some(view) => {
                service.clear(storage, user_id, session_id).await?;
                Ok(("Goal cleared.".to_string(), Some(goal_payload(&view))))
            }
        },
    };
    match result {
        Ok(ok) => Ok(ok),
        Err(error) => {
            if error.downcast_ref::<GoalError>().is_some() {
                Ok((
                    "The goal command is not valid for the current state. Run /goal to view \
                     available commands."
                        .to_string(),
                    current_payload,
                ))
            } else {
                Err(error)
            }
        }
    }
}
