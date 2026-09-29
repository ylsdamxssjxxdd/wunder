//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    AgentRecord, AgentSettingsEdit, CronRecord, CronRunRecord, DesktopSettings, Directory,
    FileRecord, NativeCronJobEdit,
    LanPeerRecord, LanSettings, ModelEdit, ModelRecord, NativeChatAttachment, NativeChatEvent,
    NativeChatInput, NativeDesktop, NativeMessage, NativeProfile, NativeSession, NativeStream,
    ToolRecord, WorldContact, WorldEventFeed, WorldGroup, WorldGroupDetail, WorldGroupMember,
    WorldMessage, WorldMessageTracker,
};
pub use runtime::DesktopRuntime;
