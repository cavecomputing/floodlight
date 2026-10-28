//! The graph every live screen draws: one value across the runs, pings or intervals, a dot
//! each on a round scale over a baseline. The speed test, ping and LAN speed screens all draw
//! theirs through it.

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
/// Columns from one point to the next.
const STEP: u16 = 2;
/// The mark a value makes: a small dot, not the `●` the status bar uses.
const DOT: &str = "•";

/// One result across the runs: a dot a run, two columns apart, as high as its value on a round
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
    let fit = plot_width.div_ceil(STEP) as usize;
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
            buffer.set_string(area.x + SCALE + i as u16 * STEP, y, DOT, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::TERMINAL;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Every row of the drawn graph as its symbols.
    fn draw(values: &[Option<f32>]) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(30, PLOT_ROWS + 3)).unwrap();
        terminal
            .draw(|frame| {
                graph(
                    frame,
                    frame.area(),
                    "ms",
                    values,
                    TERMINAL.good(),
                    &TERMINAL,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_dot_a_value_and_a_missing_one_leaves_a_gap() {
        let rows = draw(&[Some(10.0), Some(40.0), None, Some(40.0)]);
        let dots = |x: u16| -> usize {
            rows[2..2 + PLOT_ROWS as usize]
                .iter()
                .filter(|row| row.chars().nth((SCALE + x) as usize) == Some('•'))
                .count()
        };
        assert_eq!(
            (dots(0), dots(STEP), dots(2 * STEP), dots(3 * STEP)),
            (1, 1, 0, 1)
        );
    }

    #[test]
    fn zero_sits_on_the_baseline_and_the_top_on_the_top_row() {
        let rows = draw(&[Some(0.0), Some(50.0)]);
        assert!(rows[2].starts_with("   50"));
        assert_eq!(rows[2].chars().nth((SCALE + STEP) as usize), Some('•'));
        let bottom = &rows[2 + PLOT_ROWS as usize - 1];
        assert!(bottom.starts_with("    0"));
        assert_eq!(bottom.chars().nth(SCALE as usize), Some('•'));
    }
}
