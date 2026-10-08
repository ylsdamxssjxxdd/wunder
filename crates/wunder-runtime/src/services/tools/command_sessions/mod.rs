mod broker;
mod desktop;
#[cfg(all(windows, feature = "terminal-pty"))]
pub(crate) mod terminal_pty;
mod tracker;
mod types;

pub(crate) use broker::CommandProcessHandle;
pub use broker::CommandSessionBroker;
pub use desktop::{
    find_winpty_library, DesktopTerminalBackend, DesktopTerminalFrame, DesktopTerminalService,
    DesktopTerminalSnapshot, DesktopTerminalStartSpec, DesktopTerminalStatus,
};
pub(crate) use tracker::CommandSessionTracker;
pub(crate) use types::{CommandSessionLaunchMode, CommandSessionStatus, CommandSessionStream};
