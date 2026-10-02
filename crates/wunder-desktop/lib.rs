//! Native desktop runtime entry point.
//!
//! The Slint process can link this crate directly. The bridge binary remains a
//! compatibility transport for the server-style desktop and older clients.

pub mod args;
pub mod native;
pub mod runtime;

pub use native::{
    AgentImportOutcome, AgentRecord, AgentSettingsEdit, ChannelAccountCard, ChannelAccountListing,
    ChannelBindingCard, ChannelCatalogItem, ChannelLogEntry, CronRecord, CronRunRecord,
    DesktopSettings, Directory, FileRecord, LanPeerRecord, LanSettings, ModelEdit,
    ModelProbeOutcome, ModelRecord, NativeChannelAccountEdit, NativeChannelBindingEdit,
    PlazaImportOutcome, PlazaItemCard,
    NativeChatAttachment, NativeChatEvent, NativeChatInput, NativeCronJobEdit, NativeDesktop,
    NativeMessage, NativeProfile, NativeSession, NativeStream, PromptPackInfo,
    PromptSegmentContent, ToolRecord, WorldContact, WorldEventFeed, WorldGroup, WorldGroupDetail,
    WeixinQrLoginStart, WeixinQrLoginStatus, WorldGroupMember, WorldMessage,
    WorldMessageTracker, WORKER_CARD_SCHEMA_VERSION,
};
pub use runtime::DesktopRuntime;
