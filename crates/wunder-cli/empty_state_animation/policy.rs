//! What the banner is allowed to do on this terminal: how big it may get, whether it
//! may move, and which colours it may use.

use super::renderer::{MAX_COLUMNS, MAX_ROWS};

/// A round wheel needs four cell columns per cell row: a terminal row is about twice as
/// tall as a column is wide, and braille splits that row into four dot rows.
const COLUMNS_PER_ROW: usize = 4;

/// Below this the wheel turns into unreadable texture, so the banner yields to text.
const MIN_COLUMNS: u16 = 30;
const MIN_ROWS: u16 = 8;

/// Fallback material when the terminal reports nothing: the Rust orange the icon uses, so
/// the banner and the exe icon are the same object in the same colour.
const DEFAULT_FOREGROUND: [u8; 3] = [247, 76, 0];
const DEFAULT_BACKGROUND: [u8; 3] = [15, 20, 37];

/// How the banner should present itself on this frame. Whether it presents at all is the
/// caller's question (`EmptyStateAnimation::is_eligible`), so there is no hidden variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presentation {
    /// Draw the centred pose and ask for no further frames.
    Static,
    /// Draw and keep asking for frames until the sweep holds.
    Animated,
}

/// Stage grid for a terminal `width_cells` wide offering `budget_rows`, or `None` when
/// the wheel would be too small to read.
pub(crate) fn fit(width_cells: u16, budget_rows: u16) -> Option<(u16, u16)> {
    let usable = width_cells.saturating_sub(2);
    let mut width = usable.min(MAX_COLUMNS);
    let mut height = width / COLUMNS_PER_ROW as u16;
    let tallest = budget_rows.min(MAX_ROWS);
    if height > tallest {
        height = tallest;
        width = (tallest * COLUMNS_PER_ROW as u16).min(width);
    }
    if width < MIN_COLUMNS || height < MIN_ROWS {
        return None;
    }
    Some((width, height))
}

/// `NO_COLOR` is the one convention every terminal that cares about colour access honours.
pub(crate) fn colours_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

/// Continuous redraws are off by request, or where the terminal cannot be assumed to take them.
pub(crate) fn motion_reduced() -> bool {
    let requested = ["WUNDER_REDUCED_MOTION", "WUNDER_NO_ANIMATION"]
        .iter()
        .any(|key| match std::env::var(key) {
            Ok(value) => {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            }
            Err(_) => false,
        });
    let dumb = std::env::var("TERM")
        .map(|value| value.trim() == "dumb")
        .unwrap_or(false);
    requested || dumb
}

/// Terminal material. A real capability probe belongs here; until the CLI has one these
/// defaults keep the wheel lit the same way everywhere.
pub(crate) fn terminal_material() -> ([u8; 3], [u8; 3]) {
    (DEFAULT_FOREGROUND, DEFAULT_BACKGROUND)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_is_square_in_screen_terms_and_capped() {
        let (width, height) = fit(120, 40).expect("fit");
        assert_eq!(width, height * COLUMNS_PER_ROW as u16);
        assert!(width <= MAX_COLUMNS && height <= MAX_ROWS);
    }

    #[test]
    fn fit_shrinks_with_the_terminal_and_refuses_when_tiny() {
        let (width, _) = fit(36, 20).expect("narrow fit");
        assert!(width <= 36);
        assert!(fit(20, 20).is_none(), "too narrow");
        assert!(fit(120, 4).is_none(), "too short");
    }

    #[test]
    fn a_short_budget_narrows_the_wheel_instead_of_clipping_it() {
        let (width, height) = fit(120, 8).expect("short budget");
        assert_eq!(height, 8);
        assert_eq!(width, 8 * COLUMNS_PER_ROW as u16);
    }

    #[test]
    fn a_one_row_budget_can_never_produce_a_stage() {
        assert_eq!(fit(200, 0), None);
        assert_eq!(fit(200, 1), None);
    }
}
