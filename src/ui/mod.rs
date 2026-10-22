//! Everything on screen: the search screen (ratatui) and the plain table.

pub mod app;
pub mod plain;
pub mod status_bar;
pub mod theme;

/// Columns in from the left edge, like the 16px inset of a desktop panel.
pub const INSET: u16 = 2;
