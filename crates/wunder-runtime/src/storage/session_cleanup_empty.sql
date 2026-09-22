c.last_message_at > c.created_at
AND NOT EXISTS (SELECT 1 FROM chat_history h WHERE h.session_id = c.session_id AND h.user_id = c.user_id)
AND NOT EXISTS (SELECT 1 FROM model_context_entries e WHERE e.session_id = c.session_id AND e.user_id = c.user_id)
AND NOT EXISTS (SELECT 1 FROM stream_events e WHERE e.session_id = c.session_id)
AND NOT EXISTS (SELECT 1 FROM monitor_sessions m WHERE m.session_id = c.session_id)
AND NOT EXISTS (SELECT 1 FROM tool_logs t WHERE t.session_id = c.session_id AND t.user_id = c.user_id)
AND NOT EXISTS (SELECT 1 FROM artifact_logs a WHERE a.session_id = c.session_id AND a.user_id = c.user_id)
AND NOT EXISTS (SELECT 1 FROM cron_jobs j WHERE j.session_id = c.session_id AND j.user_id = c.user_id)
AND NOT EXISTS (SELECT 1 FROM session_goals g WHERE g.session_id = c.session_id AND g.user_id = c.user_id AND g.status = 'active')
