EXISTS (SELECT 1 FROM session_locks l WHERE l.session_id = c.session_id AND (l.expires_at > :now OR l.suspended <> 0))
OR EXISTS (SELECT 1 FROM agent_tasks t WHERE t.session_id = c.session_id AND t.status IN ('pending', 'running', 'retry', 'queued', 'waiting'))
OR EXISTS (SELECT 1 FROM session_runs r WHERE r.session_id = c.session_id AND r.status IN ('pending', 'running', 'queued', 'waiting', 'cancelling'))
OR EXISTS (SELECT 1 FROM monitor_sessions m WHERE m.session_id = c.session_id AND m.status IN ('running', 'queued', 'waiting', 'cancelling'))
