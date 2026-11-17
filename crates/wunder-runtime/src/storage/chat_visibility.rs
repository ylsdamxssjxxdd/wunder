//! One SQL predicate for "a thread the user can see".
//!
//! The work catalogue (sidebar thread strip) hides three kinds of `chat_sessions`
//! rows: child threads spawned by the model or by subagent control, scheduled
//! (`cron`) trace threads, and channel conversations. Every other surface that
//! reports a thread number — most visibly the per-workspace badge — must use the
//! same predicate, otherwise a workspace claims more threads than its tree can
//! ever show (child threads of a subagent run stayed `active` forever and made
//! the badge read 3 for a single visible thread).
//!
//! The fragment is correlated on the surrounding `chat_sessions` table, so it is
//! only valid where that table is in scope unaliased. It is dialect-neutral
//! (`COALESCE`, `NOT IN`, `NOT EXISTS`) and therefore shared by SQLite and
//! Postgres.

/// Rows the work catalogue may list, as a bare boolean SQL expression.
pub(crate) const WORK_THREAD_VISIBLE_SQL: &str = "(COALESCE(parent_session_id, '') = '' OR COALESCE(spawned_by, '') NOT IN ('model', 'subagent_control')) \
     AND COALESCE(spawned_by, '') <> 'cron' \
     AND NOT EXISTS (SELECT 1 FROM channel_sessions AS channel_session \
     WHERE channel_session.session_id = chat_sessions.session_id \
     AND channel_session.user_id = chat_sessions.user_id)";

#[cfg(test)]
mod tests {
    use super::WORK_THREAD_VISIBLE_SQL;

    #[test]
    fn the_shared_predicate_keeps_all_three_exclusions() {
        for needle in [
            "parent_session_id",
            "'model'",
            "'subagent_control'",
            "'cron'",
            "channel_sessions",
        ] {
            assert!(
                WORK_THREAD_VISIBLE_SQL.contains(needle),
                "predicate lost {needle}"
            );
        }
    }
}
