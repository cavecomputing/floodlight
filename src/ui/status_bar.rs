//! The bar along the bottom: a line in `border` over `bg-page`, the sweep's state on the left
//! (or a notice for a few seconds), the keys on the right, dropped from the end when the window
//! is narrow.

use crate::ui::INSET;
use crate::ui::theme::Roles;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use std::time::Instant;

/// A few words about something that just happened, shown in place of the state.
pub struct Notice {
    pub text: String,
    /// Aqua for something that worked, orange for something that didn't.
    pub ok: bool,
    pub until: Instant,
}

pub fn draw(frame: &mut Frame, rule: Rect, bar: Rect, roles: &Roles, left: Line, hints: &[&str]) {
    let page = Style::new().bg(roles.bg_page);
    frame.render_widget(Block::new().style(page), rule);
    frame.render_widget(Block::new().style(page), bar);
    frame.render_widget(
        Paragraph::new(Span::styled("─".repeat(rule.width as usize), roles.rule())),
        rule,
    );

    let inner = Rect {
        x: bar.x + INSET,
        width: bar.width.saturating_sub(INSET * 2),
        ..bar
    };
    let taken = left.width();
    frame.render_widget(Paragraph::new(left), inner);

    let room = (inner.width as usize).saturating_sub(taken + 3);
    let mut shown = hints.len();
    while shown > 0 && hints[..shown].join(" · ").chars().count() > room {
        shown -= 1;
    }
    if shown > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(hints[..shown].join(" · "), roles.comment()))
                .right_aligned(),
            inner,
        );
    }
}
