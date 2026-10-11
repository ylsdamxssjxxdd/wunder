//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    AgentRecord, AgentSettingsEdit, ChannelAccountCard, ChannelAccountListing, ChannelBindingCard,
    ChannelCatalogItem, ChannelLogEntry, CloudStatusView, CompanionAgentOverride,
    CompanionOverlayState, CompanionPlacement, CronRecord, CronRunRecord, DesktopSettings,
    LanPeerRecord, LanSettings, ModelEdit, ModelProbeOutcome, ModelRecord,
    NativeChannelAccountEdit, NativeChannelBindingEdit, NativeChatAttachment, NativeChatEvent,
    NativeChatInput, NativeContextUsage, NativeCronJobEdit, NativeDesktop, NativeMessage,
    NativeNavigationOrder, NativePatchFile, NativePatchLine, NativeProfile, NativeQueueTurn,
    NativeSession, NativeStream, NativeSubagentCard, NativeTerminalFrame, NativeTerminalSpec,
    NativeWorkflowEntry,
    NativeWorkflowLine, NativeWorkflowSection, NativeWorkspace, PromptPackInfo,
    PromptSegmentContent, RuntimeToolStatus, ToolRecord, WorkspaceDeleteSummary, WorkspaceEdit,
    WorkspacePathReport, DEFAULT_AGENT_ID, WORKSPACE_COLORS, WORKSPACE_ICONS,
};
pub use runtime::DesktopRuntime;
