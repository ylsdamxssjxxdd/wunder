// 全局应用状态：集中初始化核心服务并管理依赖注入。

use crate::a2a_store::A2aStore;
use crate::benchmark::BenchmarkManager;
use crate::channels::{ChannelHub, ChannelHubSharedState};
use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::core::approval_registry::PendingApprovalRegistry;
use crate::cron::{CronScheduler, CronWakeSignal};
use crate::gateway::GatewayHub;
use crate::lsp::LspManager;
use crate::memory::MemoryStore;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::org_units;
use crate::services::auth_sessions::AuthSessionService;
use crate::services::beeroom_realtime::BeeroomRealtimeService;
use crate::services::bridge::BridgeRuntime;
use crate::services::external_auth::ExternalAuthCodeStore;
use crate::services::inner_visible::InnerVisibleService;
use crate::services::presence::PresenceService;
use crate::services::runtime::mission::MissionRuntime;
use crate::services::runtime::thread::ThreadRuntime;
use crate::services::swarm::SwarmService;
use crate::services::tools::command_sessions::CommandSessionBroker;
use crate::services::user_world::UserWorldService;
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{build_storage, SqliteStorage, StorageBackend};
use crate::throughput::ThroughputManager;
use crate::user_store::UserStore;
use crate::user_tools::{UserToolManager, UserToolStore};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppRuntimeProfile {
    ServerDistributed,
    DesktopEmbedded,
    CliEmbedded,
}

impl AppRuntimeProfile {
    pub const fn is_embedded(self) -> bool {
        matches!(self, Self::DesktopEmbedded | Self::CliEmbedded)
    }

    pub const fn supports_lan_overlay(self) -> bool {
        matches!(self, Self::DesktopEmbedded)
    }

    pub const fn default_seed_org_units(self) -> bool {
        matches!(self, Self::ServerDistributed)
    }

    pub const fn default_ensure_default_admin(self) -> bool {
        matches!(self, Self::ServerDistributed)
    }

    pub const fn default_spawn_gateway_maintenance(self) -> bool {
        matches!(self, Self::ServerDistributed)
    }

    pub const fn default_start_mission_runtime(self) -> bool {
        matches!(self, Self::ServerDistributed)
    }

    pub const fn default_start_thread_runtime(self) -> bool {
        matches!(self, Self::ServerDistributed | Self::DesktopEmbedded)
    }

    pub const fn default_start_cron(self) -> bool {
        matches!(self, Self::ServerDistributed | Self::DesktopEmbedded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppRuntimeCapabilities {
    pub embedded_mode: bool,
    pub thread_runtime_active: bool,
    pub mission_runtime_active: bool,
    pub gateway_maintenance_active: bool,
    pub channels_enabled: bool,
    pub channel_outbox_worker_enabled: bool,
    pub cron_active: bool,
    pub lan_overlay_supported: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AppStateInitOptions {
    pub runtime_profile: AppRuntimeProfile,
    seed_org_units: Option<bool>,
    ensure_default_admin: Option<bool>,
    spawn_gateway_maintenance: Option<bool>,
    start_mission_runtime: Option<bool>,
    start_thread_runtime: Option<bool>,
    start_cron: Option<bool>,
}

impl AppStateInitOptions {
    pub const fn server_default() -> Self {
        Self::from_profile(AppRuntimeProfile::ServerDistributed)
    }

    pub const fn cli_default() -> Self {
        Self::from_profile(AppRuntimeProfile::CliEmbedded)
    }

    pub const fn desktop_default() -> Self {
        Self::from_profile(AppRuntimeProfile::DesktopEmbedded)
    }

    pub const fn from_profile(runtime_profile: AppRuntimeProfile) -> Self {
        Self {
            runtime_profile,
            seed_org_units: None,
            ensure_default_admin: None,
            spawn_gateway_maintenance: None,
            start_mission_runtime: None,
            start_thread_runtime: None,
            start_cron: None,
        }
    }

    pub const fn resolved_seed_org_units(self) -> bool {
        match self.seed_org_units {
            Some(value) => value,
            None => self.runtime_profile.default_seed_org_units(),
        }
    }

    pub const fn resolved_ensure_default_admin(self) -> bool {
        match self.ensure_default_admin {
            Some(value) => value,
            None => self.runtime_profile.default_ensure_default_admin(),
        }
    }

    pub const fn resolved_spawn_gateway_maintenance(self) -> bool {
        match self.spawn_gateway_maintenance {
            Some(value) => value,
            None => self.runtime_profile.default_spawn_gateway_maintenance(),
        }
    }

    pub const fn resolved_start_mission_runtime(self) -> bool {
        match self.start_mission_runtime {
            Some(value) => value,
            None => self.runtime_profile.default_start_mission_runtime(),
        }
    }

    pub const fn resolved_start_thread_runtime(self) -> bool {
        match self.start_thread_runtime {
            Some(value) => value,
            None => self.runtime_profile.default_start_thread_runtime(),
        }
    }

    pub const fn resolved_start_cron(self) -> bool {
        match self.start_cron {
            Some(value) => value,
            None => self.runtime_profile.default_start_cron(),
        }
    }

    pub fn resolve_capabilities(self, config: &Config) -> AppRuntimeCapabilities {
        AppRuntimeCapabilities {
            embedded_mode: self.runtime_profile.is_embedded(),
            thread_runtime_active: self.resolved_start_thread_runtime(),
            mission_runtime_active: self.resolved_start_mission_runtime(),
            gateway_maintenance_active: self.resolved_spawn_gateway_maintenance(),
            channels_enabled: config.channels.enabled,
            channel_outbox_worker_enabled: config.channels.outbox.worker_enabled,
            cron_active: self.resolved_start_cron(),
            lan_overlay_supported: self.runtime_profile.supports_lan_overlay(),
        }
    }
}

impl Default for AppStateInitOptions {
    fn default() -> Self {
        Self::server_default()
    }
}

#[derive(Clone)]
pub struct AppKernelServices {
    pub orchestrator: Arc<Orchestrator>,
    pub thread_runtime: Arc<ThreadRuntime>,
    pub swarm_service: Arc<SwarmService>,
    pub mission_runtime: Arc<MissionRuntime>,
}

#[derive(Clone)]
pub struct AppProjectionServices {
    pub user_world: Arc<UserWorldService>,
    pub beeroom: Arc<BeeroomRealtimeService>,
}

#[derive(Clone)]
pub struct AppControlServices {
    pub auth_sessions: Arc<AuthSessionService>,
    pub presence: Arc<PresenceService>,
    pub channels: Arc<ChannelHub>,
    pub gateway: Arc<GatewayHub>,
    pub cron: Arc<CronScheduler>,
    pub approval_registry: Arc<PendingApprovalRegistry>,
    pub command_sessions: Arc<CommandSessionBroker>,
}

#[derive(Clone)]
pub struct AppState {
    pub config_store: ConfigStore,
    pub runtime_profile: AppRuntimeProfile,
    pub runtime_capabilities: AppRuntimeCapabilities,
    pub workspace: Arc<WorkspaceManager>,
    pub monitor: Arc<MonitorState>,
    pub kernel: AppKernelServices,
    pub projection: AppProjectionServices,
    pub control: AppControlServices,
    pub lsp_manager: Arc<LspManager>,
    pub memory: Arc<MemoryStore>,
    pub skills: Arc<RwLock<SkillRegistry>>,
    pub inner_visible: Arc<InnerVisibleService>,
    pub user_tool_store: Arc<UserToolStore>,
    pub user_tool_manager: Arc<UserToolManager>,
    pub user_store: Arc<UserStore>,
    pub external_auth_codes: Arc<ExternalAuthCodeStore>,
    pub throughput: ThroughputManager,
    pub benchmark: BenchmarkManager,
    pub storage: Arc<dyn StorageBackend>,
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
        let runtime_profile = options.runtime_profile;
        let runtime_capabilities = options.resolve_capabilities(&config);
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
        let user_tool_store = Arc::new(
            UserToolStore::new(&config, workspace.clone()).context("初始化用户工具存储失败")?,
        );
        let user_tool_manager = Arc::new(UserToolManager::new(user_tool_store.clone()));
        let user_store = Arc::new(UserStore::new(storage.clone()));
        let inner_visible = Arc::new(InnerVisibleService::new(
            config_store.clone(),
            workspace.clone(),
            skills.clone(),
            user_tool_store.clone(),
            user_tool_manager.clone(),
            user_store.clone(),
        ));
        let auth_sessions = Arc::new(AuthSessionService::new());
        let presence = Arc::new(PresenceService::new());
        let user_world = Arc::new(UserWorldService::new(storage.clone()));
        let beeroom_realtime = Arc::new(BeeroomRealtimeService::new(storage.clone()));
        let external_auth_codes = Arc::new(ExternalAuthCodeStore::new());
        let approval_registry = Arc::new(PendingApprovalRegistry::new());
        let command_sessions = Arc::new(CommandSessionBroker::new());

        if options.resolved_seed_org_units() {
            org_units::seed_org_units_if_empty(user_store.as_ref())
                .context("Failed to seed org unit structure")?;
        }
        if options.resolved_ensure_default_admin() {
            user_store
                .ensure_default_admin()
                .context("Failed to ensure default admin account")?;
        }

        let gateway = Arc::new(GatewayHub::new(storage.clone()));
        if options.resolved_spawn_gateway_maintenance()
            && tokio::runtime::Handle::try_current().is_ok()
        {
            gateway.clone().spawn_maintenance();
        }
        let cron_wake_signal = CronWakeSignal::new();

        let orchestrator = Arc::new(Orchestrator::new(
            config_store.clone(),
            config.clone(),
            workspace.clone(),
            monitor.clone(),
            a2a_store.clone(),
            skills.clone(),
            inner_visible.clone(),
            user_tool_manager.clone(),
            lsp_manager.clone(),
            storage.clone(),
            approval_registry.clone(),
            command_sessions.clone(),
            gateway.clone(),
            user_world.clone(),
            beeroom_realtime.clone(),
            Some(cron_wake_signal.clone()),
        ));
        let swarm_service = Arc::new(SwarmService::new(storage.clone()));
        let mission_runtime = MissionRuntime::new(
            config_store.clone(),
            user_store.clone(),
            workspace.clone(),
            monitor.clone(),
            orchestrator.clone(),
            beeroom_realtime.clone(),
        );
        if options.resolved_start_mission_runtime() {
            mission_runtime.clone().start();
        }

        let thread_runtime = ThreadRuntime::new(
            config_store.clone(),
            user_store.clone(),
            monitor.clone(),
            orchestrator.clone(),
        );
        if options.resolved_start_thread_runtime() {
            thread_runtime.clone().start();
        }

        let memory = Arc::new(MemoryStore::new(storage.clone()));
        let channels = Arc::new(ChannelHub::new(
            config_store.clone(),
            storage.clone(),
            orchestrator.clone(),
            thread_runtime.clone(),
            user_store.clone(),
            workspace.clone(),
            ChannelHubSharedState {
                monitor: monitor.clone(),
                approval_registry: approval_registry.clone(),
                bridge_runtime: BridgeRuntime {
                    config_store: config_store.clone(),
                    skills: skills.clone(),
                    user_tool_manager: user_tool_manager.clone(),
                    user_store: user_store.clone(),
                    storage: storage.clone(),
                },
            },
        ));
        let cron = CronScheduler::new(
            config_store.clone(),
            storage.clone(),
            orchestrator.clone(),
            cron_wake_signal,
            user_store.clone(),
            user_tool_manager.clone(),
            skills.clone(),
        );
        if options.resolved_start_cron() {
            cron.start();
        }

        let throughput = ThroughputManager::new();
        let benchmark = BenchmarkManager::new(
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
            runtime_profile,
            runtime_capabilities,
            workspace,
            monitor,
            kernel: AppKernelServices {
                orchestrator,
                thread_runtime,
                swarm_service,
                mission_runtime,
            },
            projection: AppProjectionServices {
                user_world,
                beeroom: beeroom_realtime,
            },
            control: AppControlServices {
                auth_sessions,
                presence,
                channels,
                gateway,
                cron,
                approval_registry,
                command_sessions,
            },
            lsp_manager,
            memory,
            skills,
            inner_visible,
            user_tool_store,
            user_tool_manager,
            user_store,
            external_auth_codes,
            throughput,
            benchmark,
            storage,
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

#[cfg(test)]
mod tests {
    use super::{AppRuntimeProfile, AppStateInitOptions};

    #[test]
    fn desktop_default_starts_cron_scheduler() {
        let options = AppStateInitOptions::desktop_default();
        assert_eq!(options.runtime_profile, AppRuntimeProfile::DesktopEmbedded);
        assert!(options.resolved_start_cron());
    }

    #[test]
    fn desktop_default_enables_embedded_thread_runtime() {
        let options = AppStateInitOptions::desktop_default();
        assert!(options.runtime_profile.is_embedded());
        assert!(options.resolved_start_thread_runtime());
        assert!(!options.resolved_start_mission_runtime());
    }

    #[test]
    fn cli_default_keeps_background_runtimes_disabled() {
        let options = AppStateInitOptions::cli_default();
        assert_eq!(options.runtime_profile, AppRuntimeProfile::CliEmbedded);
        assert!(!options.resolved_start_thread_runtime());
        assert!(!options.resolved_start_mission_runtime());
        assert!(!options.resolved_start_cron());
    }
}
