CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_time REAL NOT NULL
            );

CREATE TABLE IF NOT EXISTS user_accounts (
              user_id TEXT PRIMARY KEY,
              username TEXT NOT NULL UNIQUE,
              email TEXT,
              password_hash TEXT NOT NULL,
              roles TEXT NOT NULL,
              status TEXT NOT NULL,
              access_level TEXT NOT NULL,
              unit_id TEXT,
              quota_balance INTEGER NOT NULL DEFAULT 0,
              quota_granted_total INTEGER NOT NULL DEFAULT 0,
              quota_used_total INTEGER NOT NULL DEFAULT 0,
              last_quota_grant_date TEXT,
              experience_total INTEGER NOT NULL DEFAULT 0,
              is_demo INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_login_at REAL
            );

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_username
              ON user_accounts (username);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_email
              ON user_accounts (email);

CREATE INDEX IF NOT EXISTS idx_user_accounts_unit
              ON user_accounts (unit_id);

CREATE INDEX IF NOT EXISTS idx_user_accounts_created
              ON user_accounts (created_at);

CREATE INDEX IF NOT EXISTS idx_user_accounts_unit_created
              ON user_accounts (unit_id, created_at);

CREATE TABLE IF NOT EXISTS org_units (
              unit_id TEXT PRIMARY KEY,
              parent_id TEXT,
              name TEXT NOT NULL,
              level INTEGER NOT NULL,
              path TEXT NOT NULL,
              path_name TEXT NOT NULL,
              sort_order INTEGER NOT NULL DEFAULT 0,
              leader_ids TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_org_units_parent
              ON org_units (parent_id);

CREATE INDEX IF NOT EXISTS idx_org_units_path
              ON org_units (path);

CREATE TABLE IF NOT EXISTS external_links (
              link_id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              description TEXT NOT NULL,
              url TEXT NOT NULL,
              icon TEXT NOT NULL,
              allowed_levels TEXT NOT NULL,
              sort_order INTEGER NOT NULL DEFAULT 0,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_external_links_order
              ON external_links (enabled, sort_order, updated_at);

CREATE TABLE IF NOT EXISTS user_tokens (
              token TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL DEFAULT 'default',
              family_id TEXT,
              expires_at REAL NOT NULL,
              created_at REAL NOT NULL,
              last_used_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_user_tokens_user
              ON user_tokens (user_id);

CREATE INDEX IF NOT EXISTS idx_user_tokens_expires
              ON user_tokens (expires_at);

CREATE INDEX IF NOT EXISTS idx_user_tokens_family
              ON user_tokens (family_id);

CREATE TABLE IF NOT EXISTS user_refresh_tokens (
              refresh_token TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL,
              family_id TEXT NOT NULL,
              expires_at REAL NOT NULL,
              created_at REAL NOT NULL,
              last_used_at REAL NOT NULL,
              revoked INTEGER NOT NULL DEFAULT 0
            );

CREATE INDEX IF NOT EXISTS idx_user_refresh_tokens_family
              ON user_refresh_tokens (user_id, family_id);

CREATE INDEX IF NOT EXISTS idx_user_refresh_tokens_expires
              ON user_refresh_tokens (expires_at);

CREATE TABLE IF NOT EXISTS user_session_devices (
              family_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              device_id TEXT NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_user_session_devices_device
              ON user_session_devices (device_id);

CREATE TABLE IF NOT EXISTS user_session_scopes (
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL,
              last_login_at REAL NOT NULL,
              PRIMARY KEY (user_id, session_scope)
            );

CREATE TABLE IF NOT EXISTS user_tool_access (
              user_id TEXT PRIMARY KEY,
              allowed_tools TEXT,
              updated_at REAL NOT NULL
            );

CREATE TABLE IF NOT EXISTS chat_sessions (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              title TEXT,
              status TEXT,
              agent_id TEXT,
              workspace_id TEXT,
              tool_overrides TEXT,
              parent_session_id TEXT,
              parent_message_id TEXT,
              spawn_label TEXT,
              spawned_by TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_chat_sessions_user
              ON chat_sessions (user_id);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_tasks
              ON chat_sessions (user_id, agent_id, created_at DESC, session_id DESC);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_updated
              ON chat_sessions (user_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent
              ON chat_sessions (user_id, parent_session_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_workspace
              ON chat_sessions (user_id, workspace_id, last_message_at DESC);

CREATE TABLE IF NOT EXISTS workspaces (
              workspace_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              name TEXT NOT NULL,
              root_path TEXT NOT NULL,
              icon TEXT NOT NULL DEFAULT 'folder',
              color TEXT NOT NULL DEFAULT 'blue',
              sort_index INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE UNIQUE INDEX IF NOT EXISTS idx_workspaces_user_root
              ON workspaces (user_id, root_path);

CREATE INDEX IF NOT EXISTS idx_workspaces_user_sort
              ON workspaces (user_id, sort_index);

CREATE TABLE IF NOT EXISTS user_world_conversations (
              conversation_id TEXT PRIMARY KEY,
              conversation_type TEXT NOT NULL,
              participant_a TEXT NOT NULL,
              participant_b TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL,
              last_message_id INTEGER,
              last_message_preview TEXT
            );

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_conversations_participants
              ON user_world_conversations (participant_a, participant_b);

CREATE INDEX IF NOT EXISTS idx_user_world_conversations_updated
              ON user_world_conversations (updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_user_world_conversations_last_message
              ON user_world_conversations (last_message_at DESC);

CREATE TABLE IF NOT EXISTS user_world_groups (
              group_id TEXT PRIMARY KEY,
              conversation_id TEXT NOT NULL UNIQUE,
              group_name TEXT NOT NULL,
              owner_user_id TEXT NOT NULL,
              announcement TEXT,
              announcement_updated_at REAL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_user_world_groups_conversation
              ON user_world_groups (conversation_id);

CREATE INDEX IF NOT EXISTS idx_user_world_groups_owner
              ON user_world_groups (owner_user_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS user_world_members (
              conversation_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              peer_user_id TEXT NOT NULL,
              last_read_message_id INTEGER,
              unread_count_cache INTEGER NOT NULL DEFAULT 0,
              pinned INTEGER NOT NULL DEFAULT 0,
              muted INTEGER NOT NULL DEFAULT 0,
              updated_at REAL NOT NULL,
              PRIMARY KEY (conversation_id, user_id)
            );

CREATE INDEX IF NOT EXISTS idx_user_world_members_user_updated
              ON user_world_members (user_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_user_world_members_conversation
              ON user_world_members (conversation_id);

CREATE TABLE IF NOT EXISTS user_world_messages (
              message_id INTEGER PRIMARY KEY AUTOINCREMENT,
              conversation_id TEXT NOT NULL,
              sender_user_id TEXT NOT NULL,
              content TEXT NOT NULL,
              content_type TEXT NOT NULL,
              client_msg_id TEXT,
              created_at REAL NOT NULL
            );

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_messages_client
              ON user_world_messages (conversation_id, client_msg_id);

CREATE INDEX IF NOT EXISTS idx_user_world_messages_conversation
              ON user_world_messages (conversation_id, message_id DESC);

CREATE TABLE IF NOT EXISTS user_world_events (
              conversation_id TEXT NOT NULL,
              event_id INTEGER NOT NULL,
              event_type TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL,
              PRIMARY KEY (conversation_id, event_id)
            );

CREATE INDEX IF NOT EXISTS idx_user_world_events_created_time
              ON user_world_events (created_time);

CREATE INDEX IF NOT EXISTS idx_user_world_events_conversation
              ON user_world_events (conversation_id, event_id);

CREATE TABLE IF NOT EXISTS cron_jobs (
              job_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              agent_id TEXT,
              name TEXT,
              session_target TEXT NOT NULL,
              payload TEXT NOT NULL,
              deliver TEXT,
              enabled INTEGER NOT NULL,
              delete_after_run INTEGER NOT NULL,
              schedule_kind TEXT NOT NULL,
              schedule_at TEXT,
              schedule_every_ms INTEGER,
              schedule_cron TEXT,
              schedule_tz TEXT,
              dedupe_key TEXT,
              next_run_at REAL,
              running_at REAL,
              runner_id TEXT,
              run_token TEXT,
              heartbeat_at REAL,
              lease_expires_at REAL,
              last_run_at REAL,
              last_status TEXT,
              last_error TEXT,
              consecutive_failures INTEGER NOT NULL DEFAULT 0,
              auto_disabled_reason TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_cron_jobs_user
              ON cron_jobs (user_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_cron_jobs_next
              ON cron_jobs (enabled, next_run_at);

CREATE INDEX IF NOT EXISTS idx_cron_jobs_dedupe
              ON cron_jobs (user_id, dedupe_key);

CREATE INDEX IF NOT EXISTS idx_cron_jobs_session
              ON cron_jobs (user_id, session_id);

CREATE TABLE IF NOT EXISTS cron_runs (
              run_id TEXT PRIMARY KEY,
              job_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              session_id TEXT,
              agent_id TEXT,
              trigger TEXT,
              status TEXT,
              summary TEXT,
              error TEXT,
              duration_ms INTEGER,
              created_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_cron_runs_job
              ON cron_runs (job_id, created_at);

CREATE INDEX IF NOT EXISTS idx_cron_runs_user
              ON cron_runs (user_id, created_at);

CREATE TABLE IF NOT EXISTS channel_accounts (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              config TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id)
            );

CREATE INDEX IF NOT EXISTS idx_channel_accounts_status
              ON channel_accounts (status);

CREATE TABLE IF NOT EXISTS channel_bindings (
              binding_id TEXT PRIMARY KEY,
              channel TEXT,
              account_id TEXT,
              peer_kind TEXT,
              peer_id TEXT,
              agent_id TEXT,
              tool_overrides TEXT,
              priority INTEGER NOT NULL DEFAULT 0,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_channel_bindings_match
              ON channel_bindings (channel, account_id, peer_kind, peer_id, priority);

CREATE TABLE IF NOT EXISTS channel_user_bindings (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id, peer_kind, peer_id)
            );

CREATE INDEX IF NOT EXISTS idx_channel_user_bindings_user
              ON channel_user_bindings (user_id);

CREATE TABLE IF NOT EXISTS channel_sessions (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              thread_id TEXT NOT NULL DEFAULT '',
              session_id TEXT NOT NULL,
              agent_id TEXT,
              user_id TEXT NOT NULL,
              tts_enabled INTEGER,
              tts_voice TEXT,
              metadata TEXT,
              last_message_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id, peer_kind, peer_id, thread_id)
            );

CREATE INDEX IF NOT EXISTS idx_channel_sessions_session
              ON channel_sessions (session_id);

CREATE INDEX IF NOT EXISTS idx_channel_sessions_peer
              ON channel_sessions (channel, account_id, peer_id);

CREATE TABLE IF NOT EXISTS channel_messages (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              thread_id TEXT,
              session_id TEXT,
              message_id TEXT,
              sender_id TEXT,
              message_type TEXT,
              payload TEXT NOT NULL,
              raw_payload TEXT,
              created_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_channel_messages_session
              ON channel_messages (session_id, id);

CREATE INDEX IF NOT EXISTS idx_channel_messages_peer
              ON channel_messages (channel, account_id, peer_id, id);

CREATE TABLE IF NOT EXISTS channel_outbox (
              outbox_id TEXT PRIMARY KEY,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              thread_id TEXT,
              payload TEXT NOT NULL,
              status TEXT NOT NULL,
              retry_count INTEGER NOT NULL,
              retry_at REAL NOT NULL,
              last_error TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              delivered_at REAL
            );

CREATE INDEX IF NOT EXISTS idx_channel_outbox_status
              ON channel_outbox (status, retry_at);

CREATE INDEX IF NOT EXISTS idx_channel_outbox_peer
              ON channel_outbox (channel, account_id, peer_id);

CREATE TABLE IF NOT EXISTS bridge_centers (
              center_id TEXT PRIMARY KEY,
              name TEXT NOT NULL UNIQUE,
              code TEXT NOT NULL UNIQUE,
              description TEXT,
              owner_user_id TEXT NOT NULL,
              status TEXT NOT NULL,
              default_preset_agent_name TEXT NOT NULL,
              target_unit_id TEXT,
              default_identity_strategy TEXT NOT NULL,
              username_policy TEXT NOT NULL,
              password_policy TEXT NOT NULL,
              settings_json TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_bridge_centers_status
              ON bridge_centers (status, updated_at);

CREATE TABLE IF NOT EXISTS bridge_center_accounts (
              center_account_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              default_preset_agent_name_override TEXT,
              identity_strategy TEXT,
              thread_strategy TEXT,
              reply_strategy TEXT,
              fallback_policy TEXT NOT NULL,
              provider_caps_json TEXT,
              status_reason TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              UNIQUE (channel, account_id),
              UNIQUE (center_id, channel, account_id)
            );

CREATE INDEX IF NOT EXISTS idx_bridge_center_accounts_center
              ON bridge_center_accounts (center_id, updated_at);

CREATE TABLE IF NOT EXISTS bridge_user_routes (
              route_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              center_account_id TEXT NOT NULL,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              external_identity_key TEXT NOT NULL,
              external_user_key TEXT,
              external_display_name TEXT,
              external_peer_id TEXT,
              external_sender_id TEXT,
              external_thread_id TEXT,
              external_profile_json TEXT,
              wunder_user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              agent_name TEXT NOT NULL,
              user_created INTEGER NOT NULL DEFAULT 0,
              agent_created INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL,
              last_session_id TEXT,
              last_error TEXT,
              first_seen_at REAL NOT NULL,
              last_seen_at REAL NOT NULL,
              last_inbound_at REAL,
              last_outbound_at REAL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              UNIQUE (center_account_id, external_identity_key)
            );

CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_center
              ON bridge_user_routes (center_id, status, last_seen_at);

CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_user
              ON bridge_user_routes (wunder_user_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_agent
              ON bridge_user_routes (agent_id, updated_at);

CREATE TABLE IF NOT EXISTS bridge_delivery_logs (
              delivery_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              center_account_id TEXT NOT NULL,
              route_id TEXT,
              direction TEXT NOT NULL,
              stage TEXT NOT NULL,
              provider_message_id TEXT,
              session_id TEXT,
              status TEXT NOT NULL,
              summary TEXT NOT NULL,
              payload_json TEXT,
              created_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_bridge_delivery_logs_center
              ON bridge_delivery_logs (center_id, created_at);

CREATE INDEX IF NOT EXISTS idx_bridge_delivery_logs_route
              ON bridge_delivery_logs (route_id, created_at);

CREATE TABLE IF NOT EXISTS bridge_route_audit_logs (
              audit_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              route_id TEXT,
              actor_type TEXT NOT NULL,
              actor_id TEXT NOT NULL,
              action TEXT NOT NULL,
              detail_json TEXT,
              created_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_bridge_route_audit_logs_center
              ON bridge_route_audit_logs (center_id, created_at);

CREATE INDEX IF NOT EXISTS idx_bridge_route_audit_logs_route
              ON bridge_route_audit_logs (route_id, created_at);

CREATE TABLE IF NOT EXISTS gateway_clients (
              connection_id TEXT PRIMARY KEY,
              role TEXT NOT NULL,
              user_id TEXT,
              node_id TEXT,
              scopes TEXT,
              caps TEXT,
              commands TEXT,
              client_info TEXT,
              status TEXT NOT NULL,
              connected_at REAL NOT NULL,
              last_seen_at REAL NOT NULL,
              disconnected_at REAL
            );

CREATE INDEX IF NOT EXISTS idx_gateway_clients_status
              ON gateway_clients (status, role);

CREATE INDEX IF NOT EXISTS idx_gateway_clients_node
              ON gateway_clients (node_id, status);

CREATE TABLE IF NOT EXISTS gateway_nodes (
              node_id TEXT PRIMARY KEY,
              name TEXT,
              device_fingerprint TEXT,
              status TEXT NOT NULL,
              caps TEXT,
              commands TEXT,
              permissions TEXT,
              metadata TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_seen_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_gateway_nodes_status
              ON gateway_nodes (status);

CREATE TABLE IF NOT EXISTS gateway_node_tokens (
              token TEXT PRIMARY KEY,
              node_id TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_used_at REAL
            );

CREATE INDEX IF NOT EXISTS idx_gateway_node_tokens_node
              ON gateway_node_tokens (node_id, status);

CREATE TABLE IF NOT EXISTS media_assets (
              asset_id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              url TEXT NOT NULL,
              mime TEXT,
              size INTEGER,
              hash TEXT,
              source TEXT,
              created_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_media_assets_hash
              ON media_assets (hash);

CREATE TABLE IF NOT EXISTS speech_jobs (
              job_id TEXT PRIMARY KEY,
              job_type TEXT NOT NULL,
              status TEXT NOT NULL,
              input_text TEXT,
              input_url TEXT,
              output_text TEXT,
              output_url TEXT,
              model TEXT,
              error TEXT,
              retry_count INTEGER NOT NULL,
              next_retry_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              metadata TEXT
            );

CREATE INDEX IF NOT EXISTS idx_speech_jobs_status
              ON speech_jobs (job_type, status, next_retry_at);

CREATE TABLE IF NOT EXISTS cloud_devices (
              device_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              client TEXT NOT NULL,
              name TEXT NOT NULL,
              os TEXT,
              arch TEXT,
              app_version TEXT,
              last_seen_at REAL,
              created_at REAL,
              revoked INTEGER NOT NULL DEFAULT 0
            , node_secret_hash TEXT, secret_version INTEGER NOT NULL DEFAULT 0, interlink_enabled INTEGER, capabilities TEXT, policy_overrides TEXT, tunnel_connected INTEGER NOT NULL DEFAULT 0, last_tunnel_at REAL, secret_rotated_at REAL);

CREATE INDEX IF NOT EXISTS idx_cloud_devices_user_seen
              ON cloud_devices (user_id, last_seen_at);

CREATE TABLE IF NOT EXISTS cloud_call_records (
              call_id TEXT PRIMARY KEY,
              device_id TEXT,
              user_id TEXT,
              client TEXT,
              local_session_id TEXT,
              model TEXT,
              provider TEXT,
              status TEXT,
              quota_consumed INTEGER NOT NULL DEFAULT 0,
              queue_waited_ms INTEGER,
              prompt_tokens INTEGER,
              completion_tokens INTEGER,
              started_at REAL,
              finished_at REAL,
              duration_ms INTEGER,
              error_summary TEXT
            );

CREATE INDEX IF NOT EXISTS idx_cloud_calls_user_started
              ON cloud_call_records (user_id, started_at);

CREATE INDEX IF NOT EXISTS idx_cloud_calls_device_started
              ON cloud_call_records (device_id, started_at);

CREATE TABLE IF NOT EXISTS cloud_device_logs (
              device_id TEXT NOT NULL,
              seq INTEGER NOT NULL,
              user_id TEXT,
              client TEXT,
              level TEXT,
              category TEXT,
              event TEXT,
              message TEXT,
              local_session_id TEXT,
              created_at REAL,
              PRIMARY KEY (device_id, seq)
            );

CREATE INDEX IF NOT EXISTS idx_cloud_device_logs_user_created
              ON cloud_device_logs (user_id, created_at);

CREATE TABLE IF NOT EXISTS interlink_channels (
              channel_id TEXT PRIMARY KEY,
              device_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              client TEXT,
              instance_id TEXT NOT NULL DEFAULT 'local',
              protocol_version INTEGER NOT NULL DEFAULT 1,
              caps TEXT,
              connected_at REAL,
              last_seen_at REAL,
              rtt_ms INTEGER,
              resumed_count INTEGER NOT NULL DEFAULT 0,
              closed_reason TEXT
            );

CREATE INDEX IF NOT EXISTS idx_interlink_channels_device
              ON interlink_channels (device_id, connected_at);

CREATE INDEX IF NOT EXISTS idx_interlink_channels_user
              ON interlink_channels (user_id, connected_at);

CREATE TABLE IF NOT EXISTS interlink_node_shadows (
              device_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              revision INTEGER NOT NULL DEFAULT 0,
              summary TEXT,
              threads TEXT,
              tasks TEXT,
              workspace TEXT,
              workspaces TEXT,
              synced_at REAL
            );

CREATE INDEX IF NOT EXISTS idx_interlink_shadows_user
              ON interlink_node_shadows (user_id, synced_at);

CREATE TABLE IF NOT EXISTS interlink_commands (
              command_id TEXT PRIMARY KEY,
              direction TEXT NOT NULL,
              actor_user_id TEXT NOT NULL,
              from_node TEXT NOT NULL,
              to_node TEXT NOT NULL,
              kind TEXT NOT NULL,
              args_digest TEXT,
              approval_state TEXT NOT NULL DEFAULT 'none',
              status TEXT NOT NULL DEFAULT 'issued',
              created_at REAL,
              acked_at REAL,
              finished_at REAL,
              error_code TEXT,
              error_summary TEXT
            );

CREATE INDEX IF NOT EXISTS idx_interlink_commands_user_created
              ON interlink_commands (actor_user_id, created_at);

CREATE INDEX IF NOT EXISTS idx_interlink_commands_to_node
              ON interlink_commands (to_node, created_at);

CREATE INDEX IF NOT EXISTS idx_interlink_commands_unfinished
              ON interlink_commands (created_at, command_id)
              WHERE finished_at IS NULL AND status IN ('issued', 'queued', 'acked', 'running');

CREATE TABLE IF NOT EXISTS interlink_approvals (
              approval_id TEXT PRIMARY KEY,
              command_id TEXT NOT NULL,
              device_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              prompt TEXT,
              risk_level TEXT,
              state TEXT NOT NULL DEFAULT 'pending',
              decided_by TEXT,
              decided_at REAL,
              expires_at REAL
            );

CREATE INDEX IF NOT EXISTS idx_interlink_approvals_command
              ON interlink_approvals (command_id);

CREATE TABLE IF NOT EXISTS interlink_audit (
              seq INTEGER PRIMARY KEY AUTOINCREMENT,
              command_id TEXT,
              approval_id TEXT,
              actor TEXT,
              from_node TEXT,
              to_node TEXT,
              action TEXT NOT NULL,
              detail_digest TEXT,
              created_at REAL
            );

CREATE INDEX IF NOT EXISTS idx_interlink_audit_created
              ON interlink_audit (created_at);

CREATE INDEX IF NOT EXISTS idx_interlink_audit_actor
              ON interlink_audit (actor, created_at);

CREATE TABLE IF NOT EXISTS user_agents (
              agent_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              name TEXT NOT NULL,
              description TEXT,
              system_prompt TEXT,
              model_name TEXT,
              tool_names TEXT,
              declared_tool_names TEXT,
              declared_skill_names TEXT,
              visible_unit_ids TEXT,
              ability_items TEXT,
              access_level TEXT NOT NULL,
              approval_mode TEXT NOT NULL DEFAULT 'full_auto',
              is_shared INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL,
              icon TEXT,
              sandbox_container_id INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              preset_questions TEXT,
              preset_binding TEXT,
              silent INTEGER NOT NULL DEFAULT 0,
              prefer_mother INTEGER NOT NULL DEFAULT 0,
              preview_skill INTEGER NOT NULL DEFAULT 0
            );

CREATE INDEX IF NOT EXISTS idx_user_agents_user
              ON user_agents (user_id, updated_at);

CREATE TABLE IF NOT EXISTS user_agent_access (
              user_id TEXT PRIMARY KEY,
              allowed_agent_ids TEXT,
              blocked_agent_ids TEXT,
              updated_at REAL NOT NULL
            );

CREATE TABLE IF NOT EXISTS vector_documents (
              doc_id TEXT PRIMARY KEY,
              owner_id TEXT NOT NULL,
              base_name TEXT NOT NULL,
              doc_name TEXT NOT NULL,
              embedding_model TEXT NOT NULL,
              chunk_size INTEGER NOT NULL,
              chunk_overlap INTEGER NOT NULL,
              chunk_count INTEGER NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              content TEXT NOT NULL,
              chunks_json TEXT NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_vector_documents_owner_base
              ON vector_documents (owner_id, base_name, updated_at);

CREATE TABLE IF NOT EXISTS vector_chunks (
              chunk_id TEXT PRIMARY KEY,
              owner_id TEXT NOT NULL,
              base_name TEXT NOT NULL,
              doc_id TEXT NOT NULL,
              doc_name TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_pos INTEGER NOT NULL,
              end_pos INTEGER NOT NULL,
              content TEXT NOT NULL,
              embedding_model TEXT NOT NULL,
              vector_json TEXT NOT NULL,
              dimensions INTEGER NOT NULL,
              updated_at REAL NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_vector_chunks_lookup
              ON vector_chunks (owner_id, base_name, embedding_model, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_vector_chunks_doc
              ON vector_chunks (owner_id, base_name, doc_id);

CREATE INDEX IF NOT EXISTS idx_user_tokens_user_scope_created ON user_tokens(user_id,session_scope,created_at);
