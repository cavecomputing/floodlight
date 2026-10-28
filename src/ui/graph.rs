//! The graph every live screen draws: one value across the runs, pings or intervals, a dot
//! each on a scale from 0 in steps of 10 over a baseline, numbered along the bottom. The speed
//! test, ping and LAN speed screens all draw theirs through it.

use crate::ui::theme::Roles;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

/// Rows a graph plots in, under its title, unless it is given more.
pub const PLOT_ROWS: u16 = 6;
/// Columns a graph's scale takes, left of its points.
const SCALE: u16 = 7;
/// Columns from one point to the next.
const STEP: u16 = 2;
/// The mark a value makes: a small dot, not the `●` the status bar uses.
const DOT: &str = "•";
/// The mark a lost value leaves on the baseline, when the graph is asked to show losses.
const LOST: &str = "×";

/// One result across the runs, pings or intervals: a dot each on a scale from 0 in steps of 10
/// (or more when the rows run out), each step a faint dotted line, the numbers of the points
/// under the baseline. With a `total` (`--count`), the axis is laid out for all of them up front,
/// spread across the width, and the dots fill it from left to right. Without one, the dots sit
/// two columns apart, the lines reach only as far as they do, and the latest win when there are
/// more than fit. A missing value leaves a gap, or a `×` on the baseline when `lost` (a ping
/// that didn't come back). The plot takes every row of `area` under the title but the last,
/// which holds the numbers.
#[allow(clippy::too_many_arguments)]
pub fn graph(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    values: &[Option<f32>],
    total: Option<usize>,
    ink: Style,
    lost: bool,
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
    let rows = area.height - 3;
    let plot_width = area.width - SCALE;
    // How many points the axis holds, the columns from one to the next, and the ones skipped.
    let (points, apart, skipped) = match total {
        Some(total) if total > 1 => (total, (plot_width - 1) as f32 / (total - 1) as f32, 0),
        _ => {
            let fit = plot_width.div_ceil(STEP) as usize;
            let skipped = values.len().saturating_sub(fit);
            (values.len() - skipped, STEP as f32, skipped)
        }
    };
    let shown = &values[skipped..(skipped + points).min(values.len())];
    let (step, steps) = scale(shown, rows);
    // The same number of rows from one step to the next; any left over stay empty at the top.
    let per_step = ((rows - 1) as u32 / steps) as f32;
    let top_row = per_step * steps as f32;
    let last = area.y + 2 + rows - 1;
    let row = |value: f32| {
        let up = (value / step * per_step).round();
        last - up.clamp(0.0, top_row) as u16
    };
    let column = |i: usize| area.x + SCALE + (i as f32 * apart).round() as u16;
    let width = (column(points.max(1) - 1) - column(0) + 1) as usize;
    let buffer = frame.buffer_mut();

    for tick in 0..=steps {
        let value = step * tick as f32;
        let y = row(value);
        let label = format!("{value:>5}");
        buffer.set_string(area.x, y, label, roles.comment());
        match tick {
            0 => buffer.set_string(column(0), y, "─".repeat(width), roles.rule()),
            // Every other column, so a dot on a line still stands out.
            _ => {
                for x in (1..width).step_by(2) {
                    buffer.set_string(column(0) + x as u16, y, "·", roles.rule());
                }
            }
        }
    }

    // A number every 1, 2 or 5 times a power of ten, as often as they fit with room between.
    let digits = (skipped + points).to_string().len();
    let every = (0..)
        .flat_map(|power| [1, 2, 5].map(|times| times * 10usize.pow(power)))
        .find(|every| *every as f32 * apart >= (digits + 4) as f32)
        .expect("a big enough round number");
    for i in 0..points {
        let number = (skipped + i + 1).to_string();
        // One at the right edge ends there rather than running off it.
        let x = column(i).min(area.x + area.width - number.len() as u16);
        // The first point's 1 too, when the next number leaves it room.
        let first = i + skipped == 0 && (every - 1) as f32 * apart >= 4.0;
        if first || (skipped + i + 1).is_multiple_of(every) {
            buffer.set_string(x, last + 1, number, roles.comment());
        }
    }

    for (i, value) in shown.iter().enumerate() {
        match value {
            Some(value) => buffer.set_string(column(i), row(*value), DOT, ink),
            None if lost => buffer.set_string(column(i), last, LOST, roles.warning()),
            None => {}
        }
    }
}

/// The scale for `values` on `rows` rows, from 0: the step from one label to the next and how
/// many steps it climbs. The step is 10 while a label every two rows fits, then 20, 50, 100,
/// 200 and so on.
fn scale(values: &[Option<f32>], rows: u16) -> (f32, u32) {
    let high = values.iter().flatten().copied().fold(0.0, f32::max);
    let most_steps = ((rows - 1) / 2).max(1) as u32;
    let mut power = 1.0;
    loop {
        for times in [10.0, 20.0, 50.0] {
            let step = times * power;
            let steps = ((high / step).ceil() as u32).max(1);
            if steps <= most_steps {
                return (step, steps);
            }
        }
        power *= 10.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::TERMINAL;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Every row of the drawn graph as its symbols.
    fn draw(values: &[Option<f32>], total: Option<usize>, lost: bool) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(30, PLOT_ROWS + 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                graph(
                    frame,
                    area,
                    "ms",
                    values,
                    total,
                    TERMINAL.good(),
                    lost,
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

    /// How many times `mark` is in the column of point `i`, over the plot's rows.
    fn marks(rows: &[String], i: u16, mark: char) -> usize {
        rows[2..2 + PLOT_ROWS as usize]
            .iter()
            .filter(|row| row.chars().nth((SCALE + i * STEP) as usize) == Some(mark))
            .count()
    }

    #[test]
    fn a_dot_a_value_and_a_cross_on_the_baseline_for_a_lost_one() {
        let values = [Some(10.0), Some(40.0), None, Some(40.0)];
        let rows = draw(&values, None, true);
        let dots: Vec<usize> = (0..4).map(|i| marks(&rows, i, '•')).collect();
        assert_eq!(dots, [1, 1, 0, 1]);
        let baseline = &rows[2 + PLOT_ROWS as usize - 1];
        assert_eq!(baseline.chars().nth((SCALE + 2 * STEP) as usize), Some('×'));
        // Without losses to show, a missing value is just a gap.
        assert_eq!(marks(&draw(&values, None, false), 2, '×'), 0);
    }

    #[test]
    fn the_baseline_reaches_only_as_far_as_the_dots() {
        let rows = draw(&[Some(1.0), Some(2.0), Some(3.0)], None, false);
        let baseline: String = rows[2 + PLOT_ROWS as usize - 1]
            .chars()
            .skip(SCALE as usize)
            .collect();
        assert_eq!(baseline.trim_end().chars().count(), 2 * STEP as usize + 1);
    }

    #[test]
    fn numbers_the_points_under_the_baseline() {
        let values = vec![Some(1.0); 12];
        let rows = draw(&values, None, false);
        let numbers = &rows[PLOT_ROWS as usize + 2];
        assert_eq!(
            numbers.split_whitespace().collect::<Vec<_>>(),
            ["1", "5", "10"]
        );
        assert_eq!(numbers.find("10"), Some((SCALE + 9 * STEP) as usize));
    }

    #[test]
    fn a_count_lays_the_axis_out_up_front() {
        // Three of ten pings in: the baseline already spans the width, the dots at the left.
        let rows = draw(&[Some(1.0), Some(2.0), Some(3.0)], Some(10), false);
        let baseline: String = rows[2 + PLOT_ROWS as usize - 1]
            .chars()
            .skip(SCALE as usize)
            .collect();
        assert_eq!(baseline.trim_end().chars().count(), 30 - SCALE as usize);
        // Ten pings over 22 columns: a number every 2 or 5, the last at the right edge.
        let numbers = &rows[PLOT_ROWS as usize + 2];
        assert!(numbers.trim_end().ends_with("10"), "{numbers}");
        assert!(numbers.contains(" 1 "), "{numbers}");
    }

    #[test]
    fn the_scale_climbs_from_0_in_tens_while_they_fit() {
        assert_eq!(scale(&[Some(1.2), None, Some(3.2)], 20), (10.0, 1));
        assert_eq!(scale(&[Some(41.0), Some(52.0)], 30), (10.0, 6));
        // Too many tens for the rows: twenties, then fifties.
        assert_eq!(scale(&[Some(52.0)], 6), (50.0, 2));
        assert_eq!(scale(&[Some(124.5)], 9), (50.0, 3));
        assert_eq!(scale(&[Some(124.5)], 20), (20.0, 7));
        assert_eq!(scale(&[Some(940.0)], 6), (500.0, 2));
        assert_eq!(scale(&[None], 6), (10.0, 1));
    }
}
