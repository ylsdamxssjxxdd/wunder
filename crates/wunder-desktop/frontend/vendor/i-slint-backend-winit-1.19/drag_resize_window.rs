// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cSpell: ignore xdir ydir
use i_slint_core::lengths::LogicalLength;
use winit::window::{CursorIcon, ResizeDirection};

pub fn handle_cursor_move_for_resize(
    window: &winit::window::Window,
    position: winit::dpi::PhysicalPosition<f64>,
    current_direction: Option<ResizeDirection>,
    border_width: LogicalLength,
) -> Option<ResizeDirection> {
    if !window.is_decorated() && window.is_resizable() {
        let border_size = f64::from(border_width.get()) * window.scale_factor();
        let location = get_resize_direction(window.inner_size(), position, border_size);

        if current_direction != location {
            window.set_cursor(resize_direction_cursor_icon(location));
        }

        return location;
    }

    None
}

/// Starts a native resize gesture for `direction`. Returns false when the
/// gesture is not possible.
///
/// A maximized or fullscreen frameless window keeps WS_SIZEBOX for native snap
/// and edge resize, but starting a resize gesture from its maximized rect
/// leaves Win7 in a half-resized state that swallows later move/resize input.
/// The caller also uses the return value to decide whether the left press is
/// consumed by the border.
pub fn handle_resize(window: &winit::window::Window, direction: Option<ResizeDirection>) -> bool {
    let Some(dir) = direction else { return false };
    if window.is_maximized() || window.fullscreen().is_some() {
        return false;
    }
    if window.is_decorated() || !window.is_resizable() {
        return false;
    }
    let _ = window.drag_resize_window(dir);
    true
}

/// Get the cursor icon that corresponds to the resize direction.
fn resize_direction_cursor_icon(resize_direction: Option<ResizeDirection>) -> CursorIcon {
    match resize_direction {
        Some(resize_direction) => match resize_direction {
            ResizeDirection::East => CursorIcon::EResize,
            ResizeDirection::North => CursorIcon::NResize,
            ResizeDirection::NorthEast => CursorIcon::NeResize,
            ResizeDirection::NorthWest => CursorIcon::NwResize,
            ResizeDirection::South => CursorIcon::SResize,
            ResizeDirection::SouthEast => CursorIcon::SeResize,
            ResizeDirection::SouthWest => CursorIcon::SwResize,
            ResizeDirection::West => CursorIcon::WResize,
        },
        None => CursorIcon::Default,
    }
}

fn get_resize_direction(
    win_size: winit::dpi::PhysicalSize<u32>,
    position: winit::dpi::PhysicalPosition<f64>,
    border_size: f64,
) -> Option<ResizeDirection> {
    enum X {
        West,
        East,
        Default,
    }

    enum Y {
        North,
        South,
        Default,
    }

    let xdir = if position.x < border_size {
        X::West
    } else if position.x > (win_size.width as f64 - border_size) {
        X::East
    } else {
        X::Default
    };

    let ydir = if position.y < border_size {
        Y::North
    } else if position.y > (win_size.height as f64 - border_size) {
        Y::South
    } else {
        Y::Default
    };

    Some(match (xdir, ydir) {
        (X::West, Y::North) => ResizeDirection::NorthWest,
        (X::West, Y::South) => ResizeDirection::SouthWest,
        (X::West, Y::Default) => ResizeDirection::West,

        (X::East, Y::North) => ResizeDirection::NorthEast,
        (X::East, Y::South) => ResizeDirection::SouthEast,
        (X::East, Y::Default) => ResizeDirection::East,

        (X::Default, Y::North) => ResizeDirection::North,
        (X::Default, Y::South) => ResizeDirection::South,
        (X::Default, Y::Default) => return None,
    })
}
