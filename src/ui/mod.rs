//! Everything on screen: the sweep's search screen (ratatui), the wireless screen, the speed
//! test's, the ping, trace and LAN speed test's screens, and the plain tables.

pub mod app;
pub mod dns;
pub mod lanspeed;
pub mod ping;
pub mod plain;
pub mod speedtest;
pub mod status_bar;
pub mod theme;
pub mod trace;
pub mod wireless;

use crate::ui::theme::Roles;
use ratatui::Frame;
use ratatui::crossterm::event::{self as input, Event as Input, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Cell, Row, Table};
use std::time::{Duration, Instant};

/// Columns in from the left edge, like the 16px inset of a desktop panel.
pub const INSET: u16 = 2;
/// How often a screen looks for new results and key presses.
pub const TICK: Duration = Duration::from_millis(50);
/// The working dot's pulse, the screen's only looping motion.
const PULSE: Duration = Duration::from_millis(600);

/// The area moved in by `INSET`, where every line of text starts.
pub fn inset(area: Rect) -> Rect {
    Rect {
        x: area.x + INSET,
        width: area.width.saturating_sub(INSET),
        ..area
    }
}

/// The key pressed within a tick, if one was: every screen reads its keys this way.
pub fn pressed() -> std::io::Result<Option<KeyEvent>> {
    if input::poll(TICK)?
        && let Input::Key(key) = input::read()?
        && key.kind != KeyEventKind::Release
    {
        return Ok(Some(key));
    }
    Ok(None)
}

/// The status bar's dot while something is under way: lit and dim by turns since `opened`.
pub fn pulse(opened: Instant, roles: &Roles) -> Style {
    let lit = (opened.elapsed().as_millis() / PULSE.as_millis()).is_multiple_of(2);
    if lit { roles.active() } else { roles.comment() }
}

/// A table under a header row commented out with `#`, its rows starting two columns in from
/// the `#`, like every screen's table.
pub fn comment_table(
    frame: &mut Frame,
    area: Rect,
    roles: &Roles,
    titles: &[&'static str],
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
) {
    let header = Row::new(
        titles
            .iter()
            .map(|title| Cell::from(Span::styled(*title, roles.comment()))),
    );
    let table = Table::new(rows, widths).header(header).column_spacing(2);
    let rows = Rect {
        x: area.x + INSET + 2,
        width: area.width.saturating_sub(INSET + 2),
        ..area
    };
    frame.render_widget(table, rows);
    comment_mark(frame, area, roles);
}

/// The `#` that comments out a table's header row.
pub fn comment_mark(frame: &mut Frame, area: Rect, roles: &Roles) {
    if area.height > 0 && area.width > INSET {
        frame.buffer_mut()[(area.x + INSET, area.y)]
            .set_char('#')
            .set_style(roles.comment());
    }
}
