mod broker;
mod desktop;
#[cfg(all(windows, feature = "terminal-pty"))]
pub(crate) mod terminal_pty;
#[cfg(all(windows, feature = "terminal-pty"))]
mod terminal_conpty;
mod tracker;
mod types;

pub(crate) use broker::CommandProcessHandle;
pub use broker::CommandSessionBroker;
pub use desktop::{
    find_winpty_library, DesktopTerminalBackend, DesktopTerminalFrame, DesktopTerminalService,
    DesktopTerminalSnapshot, DesktopTerminalStartSpec, DesktopTerminalStatus, DesktopTerminalSubscription,
};
pub(crate) use tracker::CommandSessionTracker;
pub(crate) use types::{CommandSessionLaunchMode, CommandSessionStream};
