CREATE TABLE IF NOT EXISTS session_goals (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              goal_id TEXT NOT NULL,
              revision INTEGER NOT NULL,
              objective TEXT NOT NULL,
              phase TEXT NOT NULL,
              blocked_code TEXT,
              blocked_message TEXT,
              max_goal_rounds INTEGER NOT NULL,
              rounds_started INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_session_goals_user
              ON session_goals (user_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_session_goals_phase
              ON session_goals (user_id, phase, updated_at DESC);
