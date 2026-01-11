// 全局状态：配置存储、工作区管理与调度器。

use crate::a2a_store::A2aStore;
use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::evaluation_runner::EvaluationManager;
use crate::memory::MemoryStore;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{build_storage, SqliteStorage, StorageBackend};
use crate::throughput::ThroughputManager;
use crate::user_tools::{UserToolManager, UserToolStore};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

#[derive(Clone)]
pub struct AppState {
    pub config_store: ConfigStore,
    pub workspace: Arc<WorkspaceManager>,
    pub monitor: Arc<MonitorState>,
    pub orchestrator: Arc<Orchestrator>,
    pub memory: Arc<MemoryStore>,
    pub skills: Arc<RwLock<SkillRegistry>>,
    pub user_tool_store: Arc<UserToolStore>,
    pub user_tool_manager: Arc<UserToolManager>,
    pub throughput: ThroughputManager,
    pub evaluation: EvaluationManager,
}

impl AppState {
    pub fn new(config_store: ConfigStore, config: Config) -> Result<Self> {
        let storage = init_storage(&config)?;
        let workspace = Arc::new(WorkspaceManager::new(
            &config.workspace.root,
            storage.clone(),
            config.workspace.retention_days,
        ));
        let monitor = Arc::new(MonitorState::new(
            storage.clone(),
            config.observability.clone(),
            config.sandbox.clone(),
            config.workspace.root.clone(),
        ));
        let a2a_store = Arc::new(A2aStore::new());
        let skills_registry = load_skills(&config, true, true);
        let skills = Arc::new(RwLock::new(skills_registry));
        let user_tool_store =
            Arc::new(UserToolStore::new(&config).context("用户工具目录初始化失败")?);
        let user_tool_manager = Arc::new(UserToolManager::new(user_tool_store.clone()));
        let orchestrator = Arc::new(Orchestrator::new(
            config_store.clone(),
            config.clone(),
            workspace.clone(),
            monitor.clone(),
            a2a_store.clone(),
            skills.clone(),
            user_tool_manager.clone(),
            storage.clone(),
        ));
        let memory = Arc::new(MemoryStore::new(storage.clone()));
        let throughput = ThroughputManager::new();
        let evaluation = EvaluationManager::new(
            config_store.clone(),
            storage.clone(),
            workspace.clone(),
            monitor.clone(),
            orchestrator.clone(),
            skills.clone(),
            user_tool_manager.clone(),
        );
        Ok(Self {
            config_store,
            workspace,
            monitor,
            orchestrator,
            memory,
            skills,
            user_tool_store,
            user_tool_manager,
            throughput,
            evaluation,
        })
    }

    /// 重新加载技能注册表，供管理端更新后调用。
    pub async fn reload_skills(&self, config: &Config) {
        let registry = load_skills(config, true, true);
        let mut guard = self.skills.write().await;
        *guard = registry;
    }
}

fn init_storage(config: &Config) -> Result<Arc<dyn StorageBackend>> {
    let backend = config.storage.backend.trim().to_lowercase();
    let backend = if backend.is_empty() {
        "sqlite".to_string()
    } else {
        backend
    };

    match backend.as_str() {
        "sqlite" | "default" => init_storage_strict(config),
        "postgres" | "postgresql" | "pg" => init_storage_strict(config).map_err(|err| {
            anyhow!(
                "Postgres 存储初始化失败: {err}（请启动 PostgreSQL 或将 storage.backend 改为 sqlite/auto）"
            )
        }),
        "auto" => init_storage_auto(config),
        other => Err(anyhow!("未知存储后端: {other}")),
    }
}

fn init_storage_strict(config: &Config) -> Result<Arc<dyn StorageBackend>> {
    let storage = build_storage(&config.storage)?;
    storage.ensure_initialized()?;
    Ok(storage)
}

fn init_storage_auto(config: &Config) -> Result<Arc<dyn StorageBackend>> {
    match init_storage_strict(config) {
        Ok(storage) => Ok(storage),
        Err(err) => {
            warn!("Postgres 存储不可用，自动降级 SQLite: {err}");
            let sqlite = Arc::new(SqliteStorage::new(config.storage.db_path.clone()));
            sqlite.ensure_initialized()?;
            Ok(sqlite)
        }
    }
}
