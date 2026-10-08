//! Model-visible goal texts: the per-round continuation prompt, the shared
//! policy guidance injected into the system prompt, and the wrap-up notice
//! injected after an autonomous terminal update.

use super::types::BLOCKED_AFTER_CONSECUTIVE_ROUNDS;

/// Render the complete goal-round instruction retained in session history.
/// The objective is embedded as JSON data so its contents cannot escalate
/// into instructions.
pub fn render_goal_round_prompt(objective: &str, round: i64, max_goal_rounds: i64) -> String {
    format!(
        "<goal_round>\nObjective: {}\nRound: {round}/{max_goal_rounds}\n\nContinue working toward \
         the objective in this same session. Treat the current workspace, tool results, and \
         durable session state as authoritative; inspect them instead of assuming earlier \
         narration is still current. Make concrete progress and verify the result. Before \
         claiming completion, gather evidence that the whole objective is achieved, read the \
         current goal, and mark it complete. If work remains, leave the goal active for the next \
         round. Follow the configured goal-tool policy before reporting a blocker.\n</goal_round>",
        serde_json::json!(objective.trim())
    )
}

/// Render the shared goal-tool policy section for the system prompt.
pub fn render_goal_guidance() -> String {
    render_goal_guidance_with_threshold(BLOCKED_AFTER_CONSECUTIVE_ROUNDS)
}

pub fn render_goal_guidance_with_threshold(blocked_after: i64) -> String {
    format!(
        "create_goal may infer goal intent from a direct human request in any language. After \
         session resume or fork, an active goal is disarmed: when a human asks to continue or \
         resume in any wording or language, use update_goal action resume to rearm it. Mark \
         complete only when the objective is actually achieved. Mark blocked only after the same \
         blocking condition persists for at least {blocked_after} consecutive goal rounds, and \
         report that concrete condition in blocked_reason; difficulty, uncertainty, or useful \
         remaining work is not blocked."
    )
}

const GROUNDING: &str = "Report only what earlier rounds and tool results in this session \
actually establish; when a detail is not in the session, say so instead of inventing it. ";

/// Render the closing-message instruction injected after an autonomous goal
/// round reports `complete` or `blocked`, so the model still addresses the
/// user once before the turn ends.
pub fn render_wrapup_context(objective: &str, blocked_reason: Option<&str>) -> String {
    let heading = format!("Objective: {}\n", serde_json::json!(objective.trim()));
    match blocked_reason {
        None => format!(
            "<goal_complete>\n{heading}The goal is marked complete and this autonomous run is \
             ending. Write the closing message to the user now: state the outcome, summarize \
             what was done and how it was verified, and point to the concrete results (files, \
             commits, or other artifacts). {GROUNDING}Note anything the user should review or do \
             next. Address the user directly. Do not call any more tools in this run; further \
             work waits for the user's next instruction.\n</goal_complete>"
        ),
        Some(reason) => format!(
            "<goal_blocked>\n{heading}Blocked: {}\nThe goal is marked blocked and this autonomous \
             run is ending. Write the closing message to the user now: state what has been \
             completed so far, describe the concrete blocking condition and what you tried, and \
             say exactly what you need from the user to continue. {GROUNDING}Address the user \
             directly. Do not call any more tools in this run; further work waits for the user's \
             next instruction.\n</goal_blocked>",
            serde_json::json!(reason)
        ),
    }
}
