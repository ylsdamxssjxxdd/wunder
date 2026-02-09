pub const TEAM_START: &str = "team_start";
pub const TEAM_TASK_DISPATCH: &str = "team_task_dispatch";
pub const TEAM_TASK_UPDATE: &str = "team_task_update";
pub const TEAM_TASK_RESULT: &str = "team_task_result";
pub const TEAM_MERGE: &str = "team_merge";
pub const TEAM_FINISH: &str = "team_finish";
pub const TEAM_ERROR: &str = "team_error";

pub fn is_team_event(event_type: &str) -> bool {
    matches!(
        event_type,
        TEAM_START
            | TEAM_TASK_DISPATCH
            | TEAM_TASK_UPDATE
            | TEAM_TASK_RESULT
            | TEAM_MERGE
            | TEAM_FINISH
            | TEAM_ERROR
    )
}
