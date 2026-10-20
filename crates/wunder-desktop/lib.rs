//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    validate_cloud_password, AgentRecord, AgentSettingsEdit, ChannelAccountCard,
    ChannelAccountListing, ChannelBindingCard, ChannelCatalogItem, ChannelLogEntry,
    CloudStatusView, CompanionAgentOverride, CompanionOverlayState, CompanionPlacement, CronRecord,
    CronRunRecord, DesktopSettings, LanPeerRecord, LanSettings, ModelEdit, ModelProbeOutcome,
    ModelRecord, NativeChannelAccountEdit, NativeChannelBindingEdit, NativeChatAttachment,
    NativeChatEvent, NativeChatInput, NativeChatRound, NativeChatTurn, NativeContextUsage,
    NativeCronJobEdit, NativeDesktop, NativeMessage, NativeNavigationOrder, NativePatchFile,
    NativePatchLine, NativePlanStep, NativePlanView, NativeProcessItem, NativeProcessPage,
    NativeProfile, NativeQueueTurn,
    NativeSession, NativeStream, NativeSubagentCard, NativeTerminalFrame, NativeTerminalSignal,
    NativeTerminalSpec, NativeTerminalWatch, NativeThreadUpdate, NativeThreadWatch,
    NativeWorkflowEntry, NativeWorkflowLine, NativeWorkflowSection, NativeWorkspace,
    PromptPackInfo, PromptSegmentContent, RuntimeToolStatus, ToolRecord, WorkspaceDeleteSummary,
    WorkspaceEdit, WorkspacePathReport, DEFAULT_AGENT_ID, WORKSPACE_COLORS, WORKSPACE_ICONS,
};
pub use runtime::DesktopRuntime;

/// Adopt a portable installation root as WUNDER_HOME before any path is
/// resolved. Call this at the very start of a local-form process; an explicit
/// WUNDER_HOME still wins.
pub fn install_local_home_env() {
    wunder_server::local_bootstrap::install_local_home_env();
}
