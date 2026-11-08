//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod native_command_tasks;
pub mod native_host_hud;
pub mod runtime;

pub use native_command_tasks::{NativeCommandTask, NativeCommandTaskDetail};
pub use native_host_hud::{HostHud, HostHudSampler};

pub use native::{
    validate_cloud_password, AgentRecord, AgentSettingsEdit, ChannelAccountCard,
    ChannelAccountListing, ChannelBindingCard, ChannelCatalogItem, ChannelLogEntry,
    CloudStatusView, CompanionOverlayState, CompanionPlacement, CronRecord, CronRunRecord,
    DesktopSettings, LanPeerRecord, LanSettings, ModelEdit, ModelProbeOutcome, ModelRecord,
    NativeChannelAccountEdit, NativeChannelBindingEdit, NativeChatAttachment, NativeChatEvent,
    NativeChatInput, NativeChatRound, NativeChatTurn, NativeContextUsage, NativeCronJobEdit,
    NativeDesktop, NativeMessage, NativeMessageAttachment, NativeNavigationOrder, NativePatchFile,
    NativePatchLine, NativePlanStep, NativePlanView, NativeProcessItem, NativeProcessPage,
    NativeProfile, NativeQueueTurn, NativeSession, NativeStream, NativeSubagentCard,
    NativeThreadUpdate, NativeThreadWatch, NativeThreadWatchCancel, NativeWorkflowEntry,
    NativeWorkflowLine, NativeWorkflowSection, NativeWorkspace, RuntimeToolStatus, ToolRecord, WorkspaceDeleteSummary, WorkspaceEdit,
    WorkspacePathReport, CURRENT_VERSION, DEFAULT_AGENT_ID, TARGET_CLI, TARGET_DESKTOP,
    TARGET_SUPPLEMENT, UPDATE_GITHUB_REPO, UPDATE_RELAUNCH_ENV, UpdateStateView,
    WORKSPACE_COLORS, WORKSPACE_ICONS,
};
pub use wunder_server::cloud::update::{is_newer_version, AndroidRelease};
pub use runtime::DesktopRuntime;

/// Adopt a portable installation root as WUNDER_HOME before any path is
/// resolved. Call this at the very start of a local-form process; an explicit
/// WUNDER_HOME still wins.
pub fn install_local_home_env() {
    wunder_server::local_bootstrap::install_local_home_env();
}

/// Shared writable logs directory of the local form, resolved from the same
/// home the runtime uses. Crash reports and other diagnostics land here.
pub fn local_logs_dir() -> std::path::PathBuf {
    wunder_server::local_bootstrap::LocalHomeLayout::from_home(
        wunder_server::local_bootstrap::resolve_home_from_env_or_user(),
    )
    .logs
}
