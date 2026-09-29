//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    AgentRecord, AgentSettingsEdit, CronRecord, DesktopSettings, Directory, FileRecord,
    LanPeerRecord, LanSettings, ModelEdit, ModelRecord, NativeChatAttachment, NativeChatEvent,
    NativeChatInput, NativeDesktop, NativeMessage, NativeProfile, NativeSession, NativeStream,
    ToolRecord, WorldContact, WorldGroup, WorldMessage,
};
pub use runtime::DesktopRuntime;
