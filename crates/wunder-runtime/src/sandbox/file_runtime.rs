use super::{normalize_container_visible_path, SandboxContext, SandboxToolRequest, ToolResult};
use crate::a2a_store::A2aStore;
use crate::config::Config;
use crate::core::blocking;
use crate::lsp::LspManager;
use crate::services::tools::tool_error::{with_error_meta, ToolErrorMeta};
use crate::skills::SkillRegistry;
use crate::storage::{self, StorageBackend};
use crate::tools::{execute_builtin_tool, ToolContext};
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const CONTEXT_CAPACITY: usize = 128;
const CONTEXT_IDLE_TTL: Duration = Duration::from_secs(300);
const STORAGE_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct SandboxStorage {
    ready: OnceLock<Arc<dyn StorageBackend>>,
    pending: tokio::sync::Mutex<()>,
    initialization: Mutex<Option<(Instant, String)>>,
}

impl SandboxStorage {
    async fn get(self: &Arc<Self>, config: &Config) -> Result<Arc<dyn StorageBackend>, String> {
        if let Some(storage) = self.ready.get() {
            return Ok(Arc::clone(storage));
        }
        // Concurrent callers wait asynchronously instead of filling the DB pool.
        let _pending = self.pending.lock().await;
        if let Some(storage) = self.ready.get() {
            return Ok(Arc::clone(storage));
        }
        let cache = Arc::clone(self);
        let config = config.storage.clone();
        blocking::run_db("sandbox.storage.initialize", move || {
            cache.initialize(|| {
                let storage = storage::build_storage(&config)?;
                storage.ensure_initialized()?;
                Ok(storage)
            })
        })
        .await
        .map_err(|err| err.to_string())
    }

    fn initialize(
        &self,
        build: impl FnOnce() -> Result<Arc<dyn StorageBackend>>,
    ) -> Result<Arc<dyn StorageBackend>> {
        // Hold this guard in the blocking job, so caller cancellation or timeout
        // cannot start a second schema initialization while the first still runs.
        let mut failure = self.initialization.lock();
        if let Some(storage) = self.ready.get() {
            return Ok(Arc::clone(storage));
        }
        if let Some((failed_at, error)) = failure.as_ref() {
            if failed_at.elapsed() < STORAGE_RETRY_DELAY {
                return Err(anyhow::anyhow!(error.clone()));
            }
        }
        match build() {
            Ok(storage) => {
                let _ = self.ready.set(Arc::clone(&storage));
                *failure = None;
                Ok(storage)
            }
            Err(err) => {
                *failure = Some((Instant::now(), err.to_string()));
                Err(err)
            }
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct ContextKey {
    container_root: PathBuf,
    workspace_root: PathBuf,
    workspace_id: String,
}

struct FileContext {
    config: Config,
    storage: Arc<dyn StorageBackend>,
    workspace: Arc<WorkspaceManager>,
    lsp: Arc<LspManager>,
    a2a: A2aStore,
    skills: SkillRegistry,
    http: reqwest::Client,
}

struct CachedContext {
    context: Arc<FileContext>,
    last_used: Instant,
}

#[derive(Default)]
struct FileRuntime {
    storage: Arc<SandboxStorage>,
    contexts: Mutex<HashMap<ContextKey, CachedContext>>,
}

impl FileRuntime {
    async fn context(
        &self,
        key: ContextKey,
        mut config: Config,
    ) -> Result<Arc<FileContext>, String> {
        let storage = self.storage.get(&config).await?;
        let mut contexts = self.contexts.lock();
        let now = Instant::now();
        contexts.retain(|_, entry| now.duration_since(entry.last_used) < CONTEXT_IDLE_TTL);
        if let Some(entry) = contexts.get_mut(&key) {
            entry.last_used = now;
            return Ok(Arc::clone(&entry.context));
        }
        if contexts.len() >= CONTEXT_CAPACITY {
            if let Some(oldest) = contexts
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            {
                contexts.remove(&oldest);
            }
        }
        config.server.mode = "desktop".to_string();
        config.workspace.root = key.container_root.to_string_lossy().into_owned();
        // Explicitly bind every supported container id to this request's root.
        // No shared mutable routing table may be overwritten by another user.
        config.workspace.container_roots = (0..=10)
            .map(|id| (id, key.workspace_root.to_string_lossy().into_owned()))
            .collect();
        config.security.allow_paths = vec!["*".to_string()];
        config.security.deny_globs.clear();
        config.lsp.enabled = false;
        let workspace = Arc::new(WorkspaceManager::new(
            &config.workspace.root,
            Arc::clone(&storage),
            0,
            &config.workspace.container_roots,
        ));
        let context = Arc::new(FileContext {
            config,
            storage,
            lsp: LspManager::new_disabled(Arc::clone(&workspace)),
            workspace,
            a2a: A2aStore::new(),
            skills: SkillRegistry::default(),
            http: super::super::http_client().clone(),
        });
        contexts.insert(
            key,
            CachedContext {
                context: Arc::clone(&context),
                last_used: now,
            },
        );
        Ok(context)
    }
}

pub(super) async fn execute_builtin_file_tool(
    request: &SandboxToolRequest,
    context: &SandboxContext,
    args: &Value,
) -> ToolResult {
    static RUNTIME: OnceLock<FileRuntime> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(FileRuntime::default);
    let mut config = Config::default();
    configure_sandbox_file_tool_storage(&mut config);
    execute_with_runtime(runtime, config, request, context, args).await
}

async fn execute_with_runtime(
    runtime: &FileRuntime,
    config: Config,
    request: &SandboxToolRequest,
    context: &SandboxContext,
    args: &Value,
) -> ToolResult {
    let workspace_id = context
        .workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&request.user_id)
        .to_string();
    let key = ContextKey {
        container_root: context.container_root.clone(),
        workspace_root: context.workspace_root.clone(),
        workspace_id: workspace_id.clone(),
    };
    let shared = match runtime.context(key, config).await {
        Ok(context) => context,
        Err(err) => {
            return ToolResult {
                ok: false,
                data: with_error_meta(
                    json!({ "detail": err }),
                    ToolErrorMeta::new(
                        "SANDBOX_STORAGE_INIT_FAILED",
                        Some("sandbox 文件工具初始化存储失败。".to_string()),
                        true,
                        Some(1000),
                    ),
                ),
                error: "sandbox storage initialization failed".to_string(),
            }
        }
    };
    let filesystem_roots = Arc::new(vec![PathBuf::from("/")]);
    let tool_context = ToolContext {
        user_id: &request.user_id,
        session_id: &request.session_id,
        workspace_id: &workspace_id,
        agent_id: None,
        user_round: None,
        model_round: None,
        is_admin: false,
        storage: Arc::clone(&shared.storage),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace: Arc::clone(&shared.workspace),
        lsp_manager: Arc::clone(&shared.lsp),
        config: &shared.config,
        a2a_store: &shared.a2a,
        skills: &shared.skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: Some(Arc::clone(&filesystem_roots)),
        read_roots: Some(filesystem_roots),
        command_sessions: None,
        event_emitter: None,
        http: &shared.http,
    };
    let args = normalize_sandbox_file_tool_args(args);
    match execute_builtin_tool(&tool_context, &request.tool, &args).await {
        Ok(result) => ToolResult {
            ok: result.get("ok").and_then(Value::as_bool).unwrap_or(true),
            data: result,
            error: String::new(),
        },
        Err(err) => ToolResult {
            ok: false,
            data: with_error_meta(
                json!({ "detail": err.to_string() }),
                ToolErrorMeta::new(
                    "SANDBOX_FILE_TOOL_FAILED",
                    Some("sandbox 文件工具执行失败。".to_string()),
                    true,
                    Some(200),
                ),
            ),
            error: err.to_string(),
        },
    }
}

fn configure_sandbox_file_tool_storage(config: &mut Config) {
    configure_sandbox_file_tool_storage_from(config, &|name| std::env::var(name).ok());
}

fn configure_sandbox_file_tool_storage_from(
    config: &mut Config,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) {
    let backend = env_string_from(env_lookup, "WUNDER_STORAGE_BACKEND")
        .unwrap_or_else(|| "postgres".to_string());
    config.storage.backend = backend.clone();
    if let Some(db_path) = env_string_from(env_lookup, "WUNDER_SQLITE_DB_PATH") {
        config.storage.db_path = db_path;
    }
    if let Some(dsn) = env_string_from(env_lookup, "WUNDER_POSTGRES_DSN") {
        config.storage.postgres.dsn = dsn;
    } else if is_postgres_backend(&backend) {
        config.storage.postgres.dsn =
            "postgresql://wunder:wunder@wunder-postgres:5432/wunder".to_string();
    }
    if let Some(timeout_s) = env_u64_from(env_lookup, "WUNDER_POSTGRES_CONNECT_TIMEOUT_S") {
        config.storage.postgres.connect_timeout_s = timeout_s;
    }
    if let Some(pool_size) = env_usize_from(env_lookup, "WUNDER_POSTGRES_POOL_SIZE") {
        config.storage.postgres.pool_size = pool_size;
    }
}

fn env_string_from(env_lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    env_lookup(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_u64_from(env_lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<u64> {
    env_string_from(env_lookup, name).and_then(|value| value.parse::<u64>().ok())
}

fn env_usize_from(env_lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<usize> {
    env_string_from(env_lookup, name).and_then(|value| value.parse::<usize>().ok())
}

fn is_postgres_backend(backend: &str) -> bool {
    matches!(
        backend.trim().to_ascii_lowercase().as_str(),
        "postgres" | "postgresql" | "pg" | "auto"
    )
}

fn normalize_sandbox_file_tool_args(args: &Value) -> Value {
    let mut output = args.clone();
    let Value::Object(map) = &mut output else {
        return output;
    };
    for key in ["path", "workdir", "cwd", "root", "base_path"] {
        if let Some(Value::String(path)) = map.get_mut(key) {
            *path = normalize_container_visible_path(path);
        }
    }
    if let Some(Value::Array(paths)) = map.get_mut("paths") {
        for item in paths {
            if let Value::String(path) = item {
                *path = normalize_container_visible_path(path);
            }
        }
    }
    output
}

#[cfg(test)]
#[path = "file_runtime_tests.rs"]
mod concurrency_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_sandbox_file_tool_args_accepts_public_workspaces_relative_prefix() {
        let args = normalize_sandbox_file_tool_args(&json!({
            "path": "workspaces/admin__c__1/a.txt",
            "workdir": "./workspaces/admin__c__1",
            "paths": ["workspaces/admin__c__1/b.txt", "notes.txt"]
        }));

        assert_eq!(args["path"], "/workspaces/admin__c__1/a.txt");
        assert_eq!(args["workdir"], "/workspaces/admin__c__1");
        assert_eq!(args["paths"][0], "/workspaces/admin__c__1/b.txt");
        assert_eq!(args["paths"][1], "notes.txt");
    }

    #[test]
    fn sandbox_file_tool_storage_defaults_to_postgres() {
        let mut config = Config::default();
        configure_sandbox_file_tool_storage_from(&mut config, &|_| None);

        assert_eq!(config.storage.backend, "postgres");
        assert_eq!(
            config.storage.postgres.dsn,
            "postgresql://wunder:wunder@wunder-postgres:5432/wunder"
        );
    }
}
