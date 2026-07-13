use crate::storage::{TeamRunRecord, TeamTaskRecord};

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveTeamAgentEvidence {
    pub agent_id: String,
    pub session_id: String,
    pub updated_time: f64,
}

pub fn active_team_agent_evidence(
    run: &TeamRunRecord,
    tasks: &[TeamTaskRecord],
) -> Vec<ActiveTeamAgentEvidence> {
    if !matches!(
        run.status.trim().to_ascii_lowercase().as_str(),
        "queued" | "running" | "merging"
    ) {
        return Vec::new();
    }

    let mut evidence = Vec::new();
    if let Some(mother_agent_id) = run
        .mother_agent_id
        .as_deref()
        .or(run.parent_agent_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        evidence.push(ActiveTeamAgentEvidence {
            agent_id: normalize_agent_id(mother_agent_id),
            session_id: run.parent_session_id.trim().to_string(),
            updated_time: run.updated_time,
        });
    }

    for task in tasks {
        if !matches!(
            task.status.trim().to_ascii_lowercase().as_str(),
            "queued" | "running"
        ) {
            continue;
        }
        let session_id = task
            .spawned_session_id
            .as_deref()
            .or(task.target_session_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(run.parent_session_id.trim())
            .to_string();
        evidence.push(ActiveTeamAgentEvidence {
            agent_id: normalize_agent_id(&task.agent_id),
            session_id,
            updated_time: task.updated_time.max(run.updated_time),
        });
    }
    evidence
}

fn normalize_agent_id(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default") {
        String::new()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::active_team_agent_evidence;
    use crate::storage::{TeamRunRecord, TeamTaskRecord};

    fn team_run(status: &str) -> TeamRunRecord {
        TeamRunRecord {
            team_run_id: "team-a".to_string(),
            user_id: "user-a".to_string(),
            hive_id: "hive-a".to_string(),
            parent_session_id: "sess-mother".to_string(),
            parent_agent_id: Some("agent-parent".to_string()),
            mother_agent_id: Some("agent-mother".to_string()),
            strategy: "parallel".to_string(),
            status: status.to_string(),
            task_total: 2,
            task_success: 0,
            task_failed: 0,
            context_tokens_total: 0,
            context_tokens_peak: 0,
            model_round_total: 0,
            started_time: Some(1.0),
            finished_time: None,
            elapsed_s: None,
            summary: None,
            error: None,
            updated_time: 10.0,
        }
    }

    #[test]
    fn active_team_run_projects_mother_and_workers() {
        let run = team_run("running");
        let tasks = vec![
            TeamTaskRecord {
                task_id: "task-a".to_string(),
                team_run_id: run.team_run_id.clone(),
                user_id: run.user_id.clone(),
                hive_id: run.hive_id.clone(),
                agent_id: "agent-worker-a".to_string(),
                target_session_id: Some("sess-target".to_string()),
                spawned_session_id: Some("sess-spawned".to_string()),
                session_run_id: None,
                status: "queued".to_string(),
                retry_count: 0,
                priority: 0,
                started_time: None,
                finished_time: None,
                elapsed_s: None,
                result_summary: None,
                error: None,
                updated_time: 11.0,
            },
            TeamTaskRecord {
                task_id: "task-b".to_string(),
                team_run_id: run.team_run_id.clone(),
                user_id: run.user_id.clone(),
                hive_id: run.hive_id.clone(),
                agent_id: "agent-worker-b".to_string(),
                target_session_id: Some("sess-worker-b".to_string()),
                spawned_session_id: None,
                session_run_id: None,
                status: "success".to_string(),
                retry_count: 0,
                priority: 0,
                started_time: Some(2.0),
                finished_time: Some(9.0),
                elapsed_s: Some(7.0),
                result_summary: None,
                error: None,
                updated_time: 9.0,
            },
        ];

        let evidence = active_team_agent_evidence(&run, &tasks);

        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].agent_id, "agent-mother");
        assert_eq!(evidence[0].session_id, "sess-mother");
        assert_eq!(evidence[1].agent_id, "agent-worker-a");
        assert_eq!(evidence[1].session_id, "sess-spawned");
    }

    #[test]
    fn terminal_team_run_has_no_active_evidence() {
        assert!(active_team_agent_evidence(&team_run("success"), &[]).is_empty());
    }
}
