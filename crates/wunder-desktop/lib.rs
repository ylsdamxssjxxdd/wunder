//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    AgentImportOutcome, AgentRecord, AgentSettingsEdit, CronRecord, CronRunRecord,
    DesktopSettings, Directory, FileRecord, NativeCronJobEdit, ModelProbeOutcome,
    WORKER_CARD_SCHEMA_VERSION, LanPeerRecord, LanSettings, ModelEdit, ModelRecord,
    NativeChatAttachment, NativeChatEvent, PromptPackInfo, PromptSegmentContent,
    NativeChatInput, NativeDesktop, NativeMessage, NativeProfile, NativeSession, NativeStream,
    ToolRecord, WorldContact, WorldEventFeed, WorldGroup, WorldGroupDetail, WorldGroupMember,
    WorldMessage, WorldMessageTracker,
};
pub use runtime::DesktopRuntime;
