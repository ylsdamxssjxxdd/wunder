//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    AgentRecord, DesktopSettings, Directory, FileRecord, LanPeerRecord, LanSettings, ModelEdit, ModelRecord, NativeChatEvent, NativeProfile,
    NativeChatInput, NativeDesktop, NativeMessage, NativeSession, NativeStream, ToolRecord,
};
pub use runtime::DesktopRuntime;
