//! The graph every live screen draws: one value across the runs, pings or intervals, as a line
//! joining the points on a round scale over a baseline. The speed test, ping and LAN speed
//! screens all draw theirs through it.

use crate::speedtest::runs;
use crate::ui::theme::Roles;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::canvas::{self, Canvas};

/// Rows a graph plots in, under its title.
pub const PLOT_ROWS: u16 = 6;
/// Columns a graph's scale takes, left of its points.
const SCALE: u16 = 7;
/// Columns from one point to the next.
const STEP: u16 = 2;
/// Braille dots in a cell: two across, four down, so a line bends at a quarter of a row.
const DOTS_ACROSS: u16 = 2;
const DOTS_DOWN: u16 = 4;

/// One result across the runs: a point a run, two columns apart, as high as its value on a round
/// scale, each joined to the next by a line, over a `─` baseline. A run that didn't measure it
/// breaks the line and leaves a gap; the latest runs win when there are more than fit.
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

    // The plot in braille dots: a point's zero sits on the baseline, which runs through the
    // middle of the bottom row, two dots up; its top is the top row's top dot. The canvas
    // leaves cells it paints nothing in alone, so the baseline shows through the gaps.
    let dots_wide = f64::from(plot_width * DOTS_ACROSS);
    let dots_high = f64::from(PLOT_ROWS * DOTS_DOWN);
    let zero = f64::from(DOTS_DOWN / 2);
    let points: Vec<Option<(f64, f64)>> = shown
        .iter()
        .enumerate()
        .map(|(i, value)| {
            value.map(|value| {
                let x = f64::from(i as u16 * STEP * DOTS_ACROSS);
                let y = zero + f64::from(value / top).min(1.0) * (dots_high - 1.0 - zero);
                (x, y)
            })
        })
        .collect();
    let color = ink.fg.unwrap_or(Color::Reset);
    let plot = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, dots_wide - 1.0])
        .y_bounds([0.0, dots_high - 1.0])
        .paint(|ctx| {
            for pair in points.windows(2) {
                if let [Some((x1, y1)), Some((x2, y2))] = pair {
                    ctx.draw(&canvas::Line::new(*x1, *y1, *x2, *y2, color));
                }
            }
            let alone: Vec<(f64, f64)> = points.iter().flatten().copied().collect();
            ctx.draw(&canvas::Points::new(&alone, color));
        });
    frame.render_widget(
        plot,
        Rect {
            x: area.x + SCALE,
            y: first,
            width: plot_width,
            height: PLOT_ROWS,
        },
    );
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
    fn points_are_joined_and_a_missing_one_breaks_the_line() {
        let rows = draw(&[Some(10.0), Some(40.0), None, Some(40.0), Some(10.0)]);
        let plot = |x: u16| -> String {
            rows[2..2 + PLOT_ROWS as usize]
                .iter()
                .map(|row| row.chars().nth((SCALE + x) as usize).unwrap())
                .filter(|c| *c != ' ' && *c != '─')
                .collect()
        };
        // The column between the first two points carries the line climbing from 10 to 40.
        assert!(!plot(1).is_empty(), "nothing joins the first two points");
        // The lost one, two columns on, leaves its column and the ones beside it empty.
        assert!(plot(3).is_empty() && plot(4).is_empty() && plot(5).is_empty());
        assert!(!plot(7).is_empty(), "nothing joins the last two points");
    }

    #[test]
    fn zero_sits_on_the_baseline_and_the_top_on_the_top_row() {
        let rows = draw(&[Some(0.0), Some(50.0)]);
        assert!(rows[2].starts_with("   50"));
        assert!(rows[2].chars().nth((SCALE + STEP) as usize).unwrap() != ' ');
        let bottom = &rows[2 + PLOT_ROWS as usize - 1];
        assert!(bottom.starts_with("    0"));
        assert!(bottom.chars().nth(SCALE as usize).unwrap() != '─');
    }
}
