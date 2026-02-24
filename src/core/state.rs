// 全局应用状态：集中初始化核心服务并管理依赖注入。

use crate::a2a_store::A2aStore;
use crate::channels::ChannelHub;
use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::cron::CronScheduler;
use crate::evaluation_runner::EvaluationManager;
use crate::gateway::GatewayHub;
use crate::lsp::LspManager;
use crate::memory::MemoryStore;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::org_units;
use crate::services::agent_runtime::AgentRuntime;
use crate::services::external_auth::ExternalAuthCodeStore;
use crate::services::swarm::{SwarmService, TeamRunRunner};
use crate::services::user_world::UserWorldService;
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{build_storage, SqliteStorage, StorageBackend};
use crate::throughput::ThroughputManager;
use crate::user_store::UserStore;
use crate::user_tools::{UserToolManager, UserToolStore};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

#[derive(Debug, Clone, Copy)]
pub struct AppStateInitOptions {
    pub seed_org_units: bool,
    pub ensure_default_admin: bool,
    pub spawn_gateway_maintenance: bool,
    pub start_team_run_runner: bool,
    pub start_agent_runtime: bool,
    pub start_cron: bool,
}

impl AppStateInitOptions {
    pub const fn server_default() -> Self {
        Self {
            seed_org_units: true,
            ensure_default_admin: true,
            spawn_gateway_maintenance: true,
            start_team_run_runner: true,
            start_agent_runtime: true,
            start_cron: true,
        }
    }

    pub const fn cli_default() -> Self {
        Self {
            seed_org_units: false,
            ensure_default_admin: false,
            spawn_gateway_maintenance: false,
            start_team_run_runner: false,
            start_agent_runtime: false,
            start_cron: false,
        }
    }

    pub const fn desktop_default() -> Self {
        Self {
            seed_org_units: false,
            ensure_default_admin: false,
            spawn_gateway_maintenance: false,
            start_team_run_runner: false,
            start_agent_runtime: false,
            start_cron: false,
        }
    }
}

impl Default for AppStateInitOptions {
    fn default() -> Self {
        Self::server_default()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config_store: ConfigStore,
    pub workspace: Arc<WorkspaceManager>,
    pub monitor: Arc<MonitorState>,
    pub orchestrator: Arc<Orchestrator>,
    pub agent_runtime: Arc<AgentRuntime>,
    pub swarm_service: Arc<SwarmService>,
    pub team_run_runner: Arc<TeamRunRunner>,
    pub lsp_manager: Arc<LspManager>,
    pub memory: Arc<MemoryStore>,
    pub skills: Arc<RwLock<SkillRegistry>>,
    pub user_tool_store: Arc<UserToolStore>,
    pub user_tool_manager: Arc<UserToolManager>,
    pub user_store: Arc<UserStore>,
    pub user_world: Arc<UserWorldService>,
    pub external_auth_codes: Arc<ExternalAuthCodeStore>,
    pub throughput: ThroughputManager,
    pub evaluation: EvaluationManager,
    pub storage: Arc<dyn StorageBackend>,
    pub channels: Arc<ChannelHub>,
    pub gateway: Arc<GatewayHub>,
    pub cron: Arc<CronScheduler>,
}

impl AppState {
    pub fn new(config_store: ConfigStore, config: Config) -> Result<Self> {
        Self::new_with_options(config_store, config, AppStateInitOptions::server_default())
    }

    pub fn new_with_options(
        config_store: ConfigStore,
        config: Config,
        options: AppStateInitOptions,
    ) -> Result<Self> {
        let storage = init_storage(&config)?;
        let workspace = Arc::new(WorkspaceManager::new(
            &config.workspace.root,
            storage.clone(),
            config.workspace.retention_days,
            &config.workspace.container_roots,
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let monitor = Arc::new(MonitorState::new(
            storage.clone(),
            config.observability.clone(),
            config.sandbox.clone(),
            config.workspace.root.clone(),
        ));
        let a2a_store = Arc::new(A2aStore::new());
        let skills_registry = load_skills(&config, true, true, true);
        let skills = Arc::new(RwLock::new(skills_registry));
        let user_tool_store =
            Arc::new(UserToolStore::new(&config).context("初始化用户工具存储失败")?);
        let user_tool_manager = Arc::new(UserToolManager::new(user_tool_store.clone()));
        let user_store = Arc::new(UserStore::new(storage.clone()));
        let user_world = Arc::new(UserWorldService::new(storage.clone()));
        let external_auth_codes = Arc::new(ExternalAuthCodeStore::new());

        if options.seed_org_units {
            org_units::seed_org_units_if_empty(user_store.as_ref())
                .context("Failed to seed org unit structure")?;
        }
        if options.ensure_default_admin {
            user_store
                .ensure_default_admin()
                .context("Failed to ensure default admin account")?;
        }

        let gateway = Arc::new(GatewayHub::new(storage.clone()));
        if options.spawn_gateway_maintenance && tokio::runtime::Handle::try_current().is_ok() {
            gateway.clone().spawn_maintenance();
        }

        let orchestrator = Arc::new(Orchestrator::new(
            config_store.clone(),
            config.clone(),
            workspace.clone(),
            monitor.clone(),
            a2a_store.clone(),
            skills.clone(),
            user_tool_manager.clone(),
            lsp_manager.clone(),
            storage.clone(),
            gateway.clone(),
        ));
        let swarm_service = Arc::new(SwarmService::new(storage.clone()));
        let team_run_runner = TeamRunRunner::new(
            config_store.clone(),
            user_store.clone(),
            workspace.clone(),
            monitor.clone(),
            orchestrator.clone(),
        );
        if options.start_team_run_runner {
            team_run_runner.clone().start();
        }

        let agent_runtime = AgentRuntime::new(
            config_store.clone(),
            user_store.clone(),
            monitor.clone(),
            orchestrator.clone(),
        );
        if options.start_agent_runtime {
            agent_runtime.clone().start();
        }

        let memory = Arc::new(MemoryStore::new(storage.clone()));
        let channels = Arc::new(ChannelHub::new(
            config_store.clone(),
            storage.clone(),
            orchestrator.clone(),
            agent_runtime.clone(),
            user_store.clone(),
            monitor.clone(),
        ));
        let cron = CronScheduler::new(
            config_store.clone(),
            storage.clone(),
            orchestrator.clone(),
            user_store.clone(),
            user_tool_manager.clone(),
            skills.clone(),
        );
        if options.start_cron {
            cron.start();
        }

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
            agent_runtime,
            swarm_service,
            team_run_runner,
            lsp_manager,
            memory,
            skills,
            user_tool_store,
            user_tool_manager,
            user_store,
            user_world,
            external_auth_codes,
            throughput,
            evaluation,
            storage,
            channels,
            gateway,
            cron,
        })
    }

    /// 运行时重新加载技能注册表。
    pub async fn reload_skills(&self, config: &Config) {
        let registry = load_skills(config, true, true, true);
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
            anyhow!("Postgres 初始化失败: {err}。请检查 PostgreSQL 配置，或将 storage.backend 改为 sqlite/auto。")
        }),
        "auto" => init_storage_auto(config),
        other => Err(anyhow!("不支持的存储后端: {other}")),
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
            warn!("Postgres 初始化失败，已回退到 SQLite: {err}");
            let sqlite = Arc::new(SqliteStorage::new(config.storage.db_path.clone()));
            sqlite.ensure_initialized()?;
            Ok(sqlite)
        }
    }
}
