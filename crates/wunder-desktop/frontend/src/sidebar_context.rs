//! Which sidebar row owns the open context menu.
//!
//! A Slint popup takes the pointer away from the row as soon as it appears:
//! the row that was right-clicked gets its Exit in the same breath, so its
//! hover fill vanishes exactly when the menu shows up and the user cannot tell
//! which entry the menu belongs to. The row cannot notice the right press
//! itself either — the `ContextMenuArea` consumes it before the row's
//! `TouchArea` sees it — so the native shell records the pointer position of
//! the press and projects it into `MenuTarget` (ui/menu_target.slint). Every
//! row then only tests whether that point falls inside its own rectangle.
//!
//! Only Slint knows when the menu closes (item chosen, click elsewhere,
//! Escape, the window going away), and the popup exposes no callback for it,
//! so a light poll watches the window's popup list for a `Menu` entry. One
//! right press starts one timer; it stops as soon as the menu is gone, which
//! is also the moment the target highlight is cleared.

use crate::MainWindow;
use i_slint_core::window::{WindowInner, WindowKind};
use slint::{ComponentHandle, Timer, TimerMode};
use std::rc::Rc;
use std::time::Duration;

/// While a menu is up, look every 60 ms to see whether it is still there. The
/// first tick doubles as the "did a menu really open?" check for presses that
/// land where no menu belongs.
const POLL: Duration = Duration::from_millis(60);

pub struct Target {
    app: slint::Weak<MainWindow>,
    timer: Rc<Timer>,
}

pub fn install(app: &MainWindow) -> Target {
    Target {
        app: app.as_weak(),
        timer: Rc::new(Timer::default()),
    }
}

impl Target {
    /// Called by the window event filter on a right press, in window
    /// coordinates. Marks the point as the menu target right away (the menu
    /// opens in the same dispatch, so waiting for the first tick would let the
    /// hover fill disappear before the target state arrives) and starts
    /// watching for the menu's end.
    pub fn note_right_press(&self, x: f32, y: f32) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        {
            let target = app.global::<crate::MenuTarget>();
            target.set_x(x);
            target.set_y(y);
            target.set_open(true);
        }
        let app = self.app.clone();
        let timer = Rc::downgrade(&self.timer);
        self.timer.start(TimerMode::Repeated, POLL, move || {
            let Some(app) = app.upgrade() else { return };
            if menu_open(&app) {
                return;
            }
            app.global::<crate::MenuTarget>().set_open(false);
            if let Some(timer) = timer.upgrade() {
                timer.stop();
            }
        });
    }
}

/// True while any context menu popup of the main window is open. Tooltips and
/// composer popovers are different `WindowKind`s and must not keep a row lit.
fn menu_open(app: &MainWindow) -> bool {
    WindowInner::from_pub(app.window())
        .active_popups()
        .iter()
        .any(|popup| matches!(popup.window_kind, WindowKind::Menu))
}
