// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use i_slint_core::timers::Timer;

#[cfg(target_vendor = "apple")]
mod apple_display_link;

use crate::winitwindowadapter::WinitWindowAdapter;

pub fn create_frame_throttle(
    window_adapter: Weak<WinitWindowAdapter>,
    _winit_window: &winit::window::Window,
    _is_wayland: bool,
) -> Box<dyn FrameThrottle> {
    if _is_wayland {
        WinitBasedFrameThrottle::create()
    } else {
        #[cfg(target_vendor = "apple")]
        if let Some(throttle) =
            apple_display_link::try_create(window_adapter.clone(), _winit_window)
        {
            return throttle;
        }
        TimerBasedFrameThrottle::create(window_adapter)
    }
}

pub trait FrameThrottle {
    fn request_throttled_redraw(&self, winit_window: &winit::window::Window);
}

/// Pacing state shared with the frame callback through a weak handle.
struct ThrottleState {
    /// Deadline of the frame that is already queued, if any.
    scheduled: Cell<Option<Instant>>,
    /// Deadline of the most recently queued frame, so the pacing grid survives
    /// from one frame to the next.
    grid: Cell<Option<Instant>>,
}

struct TimerBasedFrameThrottle {
    window_adapter: Weak<WinitWindowAdapter>,
    state: Rc<ThrottleState>,
}

impl TimerBasedFrameThrottle {
    fn create(window_adapter: Weak<WinitWindowAdapter>) -> Box<dyn FrameThrottle> {
        Box::new(Self {
            window_adapter,
            state: Rc::new(ThrottleState {
                scheduled: Cell::new(None),
                grid: Cell::new(None),
            }),
        })
    }
}

/// Frame period for a refresh rate reported in millihertz.
///
/// Never rounded to whole milliseconds: 60 Hz is 16.667 ms, and 16 ms is 62.5 fps.
/// Pacing faster than the display makes one frame out of about every 25 land in the
/// same vertical blank, which reads as a small hitch every few hundred milliseconds
/// instead of steady motion.
fn frame_period(refresh_millihertz: u64) -> Duration {
    Duration::from_nanos(1_000_000_000_000 / refresh_millihertz)
}

fn monitor_frame_period(winit_window: &winit::window::Window) -> Duration {
    let refresh_millihertz = winit_window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        // Win7 display drivers (basic/mirror/remote adapters) report the
        // dmDisplayFrequency sentinel 0 or 1 for "hardware default". Treat any
        // implausible rate as unknown; 0 would also divide by zero.
        .filter(|&millihertz| (20_000..=500_000).contains(&millihertz))
        .unwrap_or(60_000) as u64;
    frame_period(refresh_millihertz)
}

impl FrameThrottle for TimerBasedFrameThrottle {
    fn request_throttled_redraw(&self, winit_window: &winit::window::Window) {
        let state = &self.state;
        if state.scheduled.get().is_some() {
            return;
        }

        let period = monitor_frame_period(winit_window);
        let now = Instant::now();
        // Keep advancing the previous deadline instead of restarting the clock on
        // every frame: a timer that is only re-armed after the previous frame has
        // been drawn accumulates its own wake-up delay, so the period drifts away
        // from the display's. A deadline already in the past restarts the grid
        // rather than bursting several frames to catch up.
        let deadline = state
            .grid
            .get()
            .map(|previous| previous + period)
            .filter(|deadline| *deadline > now)
            .unwrap_or_else(|| now + period);
        state.grid.set(Some(deadline));
        state.scheduled.set(Some(deadline));

        let state_weak = Rc::downgrade(state);
        let adapter_weak = self.window_adapter.clone();
        Timer::single_shot(deadline - now, move || {
            let Some(state) = state_weak.upgrade() else {
                return;
            };
            state.scheduled.set(None);
            redraw_now(&adapter_weak);
        });
    }
}

fn redraw_now(window_adapter: &Weak<WinitWindowAdapter>) {
    let Some(winit_window) = window_adapter
        .upgrade()
        .and_then(|adapter| adapter.winit_window())
    else {
        return;
    };
    winit_window.request_redraw();
}

struct WinitBasedFrameThrottle;

impl WinitBasedFrameThrottle {
    fn create() -> Box<dyn FrameThrottle> {
        Box::new(Self)
    }
}

impl FrameThrottle for WinitBasedFrameThrottle {
    fn request_throttled_redraw(&self, winit_window: &winit::window::Window) {
        winit_window.request_redraw();
    }
}
