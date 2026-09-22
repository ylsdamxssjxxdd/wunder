// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Keep embedded popups opaque over independently changing parent items,
//! without rasterizing the entire desktop window for every hover update.

use i_slint_core::item_tree::ItemTreeRc;
use i_slint_core::lengths::LogicalRect;
use i_slint_core::renderer::RendererSealed;
use i_slint_core::window::{PopupWindowLocation, WindowInner};
use i_slint_renderer_software::SoftwareRenderer;
use std::cell::RefCell;
use std::num::NonZeroU32;

pub(super) struct PopupRepaint {
    previous: RefCell<Vec<(NonZeroU32, LogicalRect)>>,
    full_repaint_check: bool,
}

impl Default for PopupRepaint {
    fn default() -> Self {
        Self {
            previous: Default::default(),
            // Diagnostic A/B switch: compare the same executable and font
            // state against the old complete-frame popup behavior.
            full_repaint_check: std::env::var_os("RCHO_SLINT_POPUP_FULL_REPAINT")
                .is_some_and(|value| value == "1"),
        }
    }
}

impl PopupRepaint {
    /// Returns (popup visible, popup stack/geometry changed). Transitions
    /// retain the full-repaint safety net, covering old bounds and shadows.
    /// Steady popups repaint their whole local rectangle, including a pixel
    /// margin for fractional DPI rounding, above any dirty parent content.
    pub(super) fn prepare(
        &self,
        window: &i_slint_core::api::Window,
        renderer: &SoftwareRenderer,
    ) -> (bool, bool) {
        let inner = WindowInner::from_pub(window);
        let popups = inner.active_popups();
        let mut previous = self.previous.borrow_mut();
        if popups.is_empty() && previous.is_empty() {
            return (false, false);
        }

        let current: Vec<_> = popups
            .iter()
            .filter_map(|popup| {
                let PopupWindowLocation::ChildWindow(origin) = &popup.location else {
                    return None;
                };
                let rect = i_slint_core::properties::evaluate_no_tracking(|| {
                    ItemTreeRc::borrow_pin(&popup.component)
                        .as_ref()
                        .item_geometry(0)
                })
                .translate(origin.to_vector());
                Some((popup.popup_id, rect))
            })
            .collect();
        let visible = !current.is_empty();
        let changed = *previous != current;
        if !changed {
            let margin = 1.0 / window.scale_factor();
            for (_, rect) in &current {
                renderer.mark_dirty_region(rect.inflate(margin, margin).into());
            }
        }
        *previous = current;
        (visible, changed || (visible && self.full_repaint_check))
    }
}
