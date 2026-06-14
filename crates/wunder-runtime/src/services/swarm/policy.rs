use super::errors::SwarmError;
use crate::storage::normalize_hive_id;

pub struct SwarmPolicyGuard;

impl SwarmPolicyGuard {
    pub fn ensure_same_hive(current_hive_id: &str, target_hive_id: &str) -> Result<(), SwarmError> {
        if normalize_hive_id(current_hive_id) == normalize_hive_id(target_hive_id) {
            return Ok(());
        }
        Err(SwarmError::denied("target is outside current hive"))
    }

    pub fn ensure_depth(depth: u32, max_depth: u32) -> Result<(), SwarmError> {
        if depth <= max_depth {
            return Ok(());
        }
        Err(SwarmError::policy_blocked("swarm depth exceeds max_depth"))
    }

    pub fn ensure_parallel_tasks(
        task_count: usize,
        max_parallel_tasks: usize,
    ) -> Result<(), SwarmError> {
        if task_count <= max_parallel_tasks {
            return Ok(());
        }
        Err(SwarmError::policy_blocked(
            "task count exceeds max_parallel_tasks_per_team",
        ))
    }

    pub fn ensure_active_runs(
        active_runs: usize,
        max_active_runs: usize,
    ) -> Result<(), SwarmError> {
        if active_runs < max_active_runs {
            return Ok(());
        }
        Err(SwarmError::policy_blocked(
            "active team runs reached max_active_team_runs",
        ))
    }

    pub fn ensure_retry(retry_count: u32, max_retry: u32) -> Result<(), SwarmError> {
        if retry_count <= max_retry {
            return Ok(());
        }
        Err(SwarmError::policy_blocked("task retry exceeds max_retry"))
    }

    pub fn sanitize_timeout_s(
        requested_timeout_s: Option<f64>,
        default_timeout_s: f64,
        hard_limit_s: f64,
    ) -> Result<f64, SwarmError> {
        let timeout = requested_timeout_s.unwrap_or(default_timeout_s);
        if !timeout.is_finite() || timeout <= 0.0 {
            return Err(SwarmError::policy_blocked("timeout must be positive"));
        }
        if timeout > hard_limit_s {
            return Err(SwarmError::policy_blocked("timeout exceeds hard limit"));
        }
        Ok(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::SwarmPolicyGuard;

    #[test]
    fn same_hive_policy_is_stable() {
        assert!(SwarmPolicyGuard::ensure_same_hive("HIVE_A", "hive_a").is_ok());
        assert!(SwarmPolicyGuard::ensure_same_hive("hive_a", "hive_b").is_err());
    }

    #[test]
    fn depth_policy_blocks_overflow() {
        assert!(SwarmPolicyGuard::ensure_depth(2, 2).is_ok());
        assert!(SwarmPolicyGuard::ensure_depth(3, 2).is_err());
    }

    #[test]
    fn parallel_and_active_policies_work() {
        assert!(SwarmPolicyGuard::ensure_parallel_tasks(4, 8).is_ok());
        assert!(SwarmPolicyGuard::ensure_parallel_tasks(9, 8).is_err());
        assert!(SwarmPolicyGuard::ensure_active_runs(3, 4).is_ok());
        assert!(SwarmPolicyGuard::ensure_active_runs(4, 4).is_err());
    }

    #[test]
    fn timeout_policy_clamps_invalid_values() {
        assert!(SwarmPolicyGuard::sanitize_timeout_s(Some(30.0), 10.0, 120.0).is_ok());
        assert!(SwarmPolicyGuard::sanitize_timeout_s(Some(0.0), 10.0, 120.0).is_err());
        assert!(SwarmPolicyGuard::sanitize_timeout_s(Some(999.0), 10.0, 120.0).is_err());
    }
}
