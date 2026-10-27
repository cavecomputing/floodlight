//! Everything on screen: the sweep's search screen (ratatui), the wireless screen, and
//! the plain tables.

pub mod app;
pub mod plain;
pub mod status_bar;
pub mod theme;
pub mod wireless;

use ratatui::layout::Rect;

/// Columns in from the left edge, like the 16px inset of a desktop panel.
pub const INSET: u16 = 2;

/// The area moved in by `INSET`, where every line of text starts.
pub fn inset(area: Rect) -> Rect {
    Rect {
        x: area.x + INSET,
        width: area.width.saturating_sub(INSET),
        ..area
    }
}
