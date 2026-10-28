//! The graph every live screen draws: one value across the runs, pings or intervals, on a round
//! scale over a baseline. The speed test, ping and LAN speed screens all draw theirs through it.

use crate::speedtest::runs;
use crate::ui::theme::Roles;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

/// Rows a graph plots in, under its title.
pub const PLOT_ROWS: u16 = 6;
/// Columns a graph's scale takes, left of its points.
const SCALE: u16 = 7;

/// One result across the runs: a `●` a run, two columns apart, as high as its value on a round
/// scale, over a `─` baseline. A run that didn't measure it leaves a gap; the latest runs win
/// when there are more than fit.
pub fn graph(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    values: &[Option<f32>],
    ink: Style,
    roles: &Roles,
) {
    if area.height < PLOT_ROWS + 3 || area.width <= SCALE + 2 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(format!("# {title}"), roles.comment())),
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        },
    );
    let plot_width = area.width - SCALE;
    let fit = plot_width.div_ceil(2) as usize;
    let shown = &values[values.len().saturating_sub(fit)..];
    let most = shown.iter().flatten().copied().fold(0.0, f32::max);
    let top = runs::scale_top(most);
    let (first, last) = (area.y + 2, area.y + 2 + PLOT_ROWS - 1);
    let buffer = frame.buffer_mut();
    buffer.set_string(area.x, first, format!("{top:>5}"), roles.comment());
    buffer.set_string(area.x, last, format!("{:>5}", 0), roles.comment());
    let baseline = "─".repeat(plot_width as usize);
    buffer.set_string(area.x + SCALE, last, baseline, roles.rule());
    for (i, value) in shown.iter().enumerate() {
        if let Some(value) = value {
            let steps = (value / top * (PLOT_ROWS - 1) as f32).round() as u16;
            let y = last - steps.min(PLOT_ROWS - 1);
            buffer.set_string(area.x + SCALE + 2 * i as u16, y, "●", ink);
        }
    }
}
