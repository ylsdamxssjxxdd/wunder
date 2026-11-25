// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use crate::items::BuiltInMouseCursor;

/// This enum represents different types of mouse cursors. It's a subset of the mouse cursors available in CSS.
/// For details and pictograms see the [MDN Documentation for cursor](https://developer.mozilla.org/en-US/docs/Web/CSS/cursor#values).
/// Depending on the backend and used OS unidirectional resize cursors may be replaced with bidirectional ones.
#[repr(C, u32)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum MouseCursorInner {
    /// One of the built-in mouse cursors.
    BuiltIn(BuiltInMouseCursor),
    /// Custom cursor from an `Image`.
    CustomMouseCursor {
        /// Image backing for this cursor.
        image: crate::graphics::Image,
        /// X pixel coordinate of the hotspot from the left edge of the image.
        ///
        /// The value is clamped to the image bounds.
        hotspot_x: i32,
        /// Y pixel coordinate of the hotspot from the top edge of the image.
        ///
        /// The value is clamped to the image bounds.
        hotspot_y: i32,
    },
}

impl Default for MouseCursorInner {
    fn default() -> Self {
        Self::BuiltIn(BuiltInMouseCursor::Default)
    }
}

/// How a window adapter shows the mouse cursor that the window passes to
/// [`WindowAdapterInternal::set_mouse_cursor`](crate::window::WindowAdapterInternal::set_mouse_cursor).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub enum MouseCursorHandling {
    /// The windowing system shows the cursor, or there is no cursor to show.
    #[default]
    Platform,
    /// The renderer draws `image` on top of the window contents, with the hotspot at the
    /// pointer position, for platforms where nothing else draws the cursor, such as when
    /// rendering directly to a display.
    ///
    /// The hotspot is in image pixels from the top-left corner, clamped to the image.
    /// Only moving the pointer then repaints just the area the cursor covers.
    Render {
        /// The image of the cursor.
        image: crate::graphics::Image,
        /// X pixel coordinate of the hotspot from the left edge of the image.
        hotspot_x: i32,
        /// Y pixel coordinate of the hotspot from the top edge of the image.
        hotspot_y: i32,
    },
}

impl MouseCursorHandling {
    /// Renders custom cursors with their own image, nothing for `MouseCursor.none`, and every
    /// other built-in cursor with `default_image`, with the hotspot at its top-left corner.
    ///
    /// Window adapters that have images for more cursor shapes can match on the cursor instead.
    pub fn render_with_default_image(
        cursor: &MouseCursorInner,
        default_image: impl FnOnce() -> crate::graphics::Image,
    ) -> Self {
        match cursor {
            MouseCursorInner::CustomMouseCursor { image, hotspot_x, hotspot_y } => {
                Self::Render { image: image.clone(), hotspot_x: *hotspot_x, hotspot_y: *hotspot_y }
            }
            MouseCursorInner::BuiltIn(BuiltInMouseCursor::None) => Self::Platform,
            _ => Self::Render { image: default_image(), hotspot_x: 0, hotspot_y: 0 },
        }
    }
}

std::thread_local! {
    /// What the pointer looked like after the last dispatched event.
    static LAST: core::cell::Cell<&'static str> = const { core::cell::Cell::new("default") };
}

/// Records the cursor a mouse event resolved to.
pub(crate) fn record(cursor: &MouseCursorInner) {
    let name = match cursor {
        MouseCursorInner::BuiltIn(cursor) => builtin_name(cursor),
        MouseCursorInner::CustomMouseCursor { .. } => "custom",
    };
    LAST.with(|last| last.set(name));
}

/// The name of the cursor the last mouse event resolved to. The windowing backend turns it into
/// an operating-system cursor, which a screenshot cannot capture; tests read it here instead.
pub fn last_mouse_cursor() -> &'static str {
    LAST.with(|last| last.get())
}

fn builtin_name(cursor: &BuiltInMouseCursor) -> &'static str {
    match cursor {
        BuiltInMouseCursor::Default => "default",
        BuiltInMouseCursor::None => "none",
        BuiltInMouseCursor::Help => "help",
        BuiltInMouseCursor::Pointer => "pointer",
        BuiltInMouseCursor::Progress => "progress",
        BuiltInMouseCursor::Wait => "wait",
        BuiltInMouseCursor::Crosshair => "crosshair",
        BuiltInMouseCursor::Text => "text",
        BuiltInMouseCursor::Alias => "alias",
        BuiltInMouseCursor::Copy => "copy",
        BuiltInMouseCursor::Move => "move",
        BuiltInMouseCursor::NoDrop => "no-drop",
        BuiltInMouseCursor::NotAllowed => "not-allowed",
        BuiltInMouseCursor::Grab => "grab",
        BuiltInMouseCursor::Grabbing => "grabbing",
        BuiltInMouseCursor::ColResize => "col-resize",
        BuiltInMouseCursor::RowResize => "row-resize",
        BuiltInMouseCursor::NResize => "n-resize",
        BuiltInMouseCursor::EResize => "e-resize",
        BuiltInMouseCursor::SResize => "s-resize",
        BuiltInMouseCursor::WResize => "w-resize",
        BuiltInMouseCursor::NeResize => "ne-resize",
        BuiltInMouseCursor::NwResize => "nw-resize",
        BuiltInMouseCursor::SeResize => "se-resize",
        BuiltInMouseCursor::SwResize => "sw-resize",
        _ => "other",
    }
}

/// Maps a custom cursor's hotspot from the source image into a buffer rendered at
/// `rendered_size` pixels, clamped to stay inside it.
pub fn scaled_hotspot(hotspot: i32, source_size: u32, rendered_size: u32) -> u32 {
    let scaled = if source_size == 0 {
        0
    } else {
        hotspot as i64 * rendered_size as i64 / source_size as i64
    };
    scaled.clamp(0, rendered_size.saturating_sub(1) as i64) as u32
}
