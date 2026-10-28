//! The graph every live screen draws: one value across the runs, pings or intervals, a dot
//! each on a scale of round steps over a baseline, numbered along the bottom. The speed test,
//! ping and LAN speed screens all draw theirs through it.

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

/// One result across the runs, pings or intervals: a dot each, two columns apart, on a scale of
/// round steps that hugs the values, each step a faint dotted line. The lines and the baseline
/// reach only as far as the dots do, and the numbers of the points run under the baseline. A
/// missing value leaves a gap, or a `×` on the baseline when `lost` (a ping that didn't come
/// back). The latest values win when there are more than fit. The plot takes every row of `area`
/// under the title but the last, which holds the numbers.
pub fn graph(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    values: &[Option<f32>],
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
    let fit = (area.width - SCALE).div_ceil(STEP) as usize;
    let skipped = values.len().saturating_sub(fit);
    let shown = &values[skipped..];
    let (bottom, step, steps) = scale(shown, rows);
    // The same number of rows from one step to the next; any left over stay empty at the top.
    let per_step = ((rows - 1) as u32 / steps) as f32;
    let top_row = per_step * steps as f32;
    let last = area.y + 2 + rows - 1;
    let row = |value: f32| {
        let up = ((value - bottom) / step * per_step).round();
        last - up.clamp(0.0, top_row) as u16
    };
    let column = |i: usize| area.x + SCALE + i as u16 * STEP;
    let width = shown.len().saturating_sub(1) * STEP as usize + 1;
    let decimals = decimals(step);
    let buffer = frame.buffer_mut();

    for tick in 0..=steps {
        let value = bottom + step * tick as f32;
        let y = row(value);
        let label = format!("{value:>5.decimals$}");
        buffer.set_string(area.x, y, label, roles.comment());
        match tick {
            0 => buffer.set_string(column(0), y, "─".repeat(width), roles.rule()),
            // Between the points, so a dot on a line still stands out.
            _ => {
                for x in (1..width).step_by(STEP as usize) {
                    buffer.set_string(column(0) + x as u16, y, "·", roles.rule());
                }
            }
        }
    }

    // A number every 1, 2 or 5 times a power of ten, as often as they fit with room between.
    let digits = (skipped + shown.len()).to_string().len();
    let every = (0..)
        .flat_map(|power| [1, 2, 5].map(|times| times * 10usize.pow(power)))
        .find(|every| every * STEP as usize >= digits + 4)
        .expect("a big enough round number");
    for i in 0..shown.len() {
        let number = skipped + i + 1;
        let x = column(i);
        let fits = x as usize + digits <= (area.x + area.width) as usize;
        if (number == 1 || number.is_multiple_of(every)) && fits {
            buffer.set_string(x, last + 1, number.to_string(), roles.comment());
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

/// The scale for `values` on `rows` rows: its bottom, the round step from one label to the
/// next (1, 2, 2.5 or 5 times a power of ten), and how many steps it climbs. It hugs the values
/// rather than starting at 0, so a few ms of jitter at 20 ms shows, and it is never narrower than
/// a fifth of the largest value, so a steady line doesn't swing from top to bottom. A label every
/// four rows or more.
fn scale(values: &[Option<f32>], rows: u16) -> (f32, f32, u32) {
    let (low, high) = values
        .iter()
        .flatten()
        .fold((f32::MAX, 0.0f32), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
    if high <= 0.0 {
        return (0.0, 1.0, 1);
    }
    let pad = (high * 0.2 - (high - low)).max(0.0) / 2.0;
    let (low, high) = ((low - pad).max(0.0), high + pad);
    let most_steps = ((rows - 1) / 4).max(2) as u32;
    let mut power = 10f32.powf(((high - low) / most_steps as f32).log10().floor());
    loop {
        for times in [1.0, 2.0, 2.5, 5.0] {
            let step = times * power;
            let bottom = (low / step).floor() * step;
            let steps = (((high - bottom) / step).ceil() as u32).max(1);
            if steps <= most_steps {
                return (bottom, step, steps);
            }
        }
        power *= 10.0;
    }
}

/// Decimal places a label needs to show `step` exactly: 0 for 5, 1 for 2.5, 2 for 0.25.
fn decimals(step: f32) -> usize {
    (0..3)
        .find(|places| {
            let shifted = step * 10f32.powi(*places as i32);
            (shifted - shifted.round()).abs() < 0.01
        })
        .unwrap_or(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::TERMINAL;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Every row of the drawn graph as its symbols.
    fn draw(values: &[Option<f32>], lost: bool) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(30, PLOT_ROWS + 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                graph(frame, area, "ms", values, TERMINAL.good(), lost, &TERMINAL)
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
        let rows = draw(&values, true);
        let dots: Vec<usize> = (0..4).map(|i| marks(&rows, i, '•')).collect();
        assert_eq!(dots, [1, 1, 0, 1]);
        let baseline = &rows[2 + PLOT_ROWS as usize - 1];
        assert_eq!(baseline.chars().nth((SCALE + 2 * STEP) as usize), Some('×'));
        // Without losses to show, a missing value is just a gap.
        assert_eq!(marks(&draw(&values, false), 2, '×'), 0);
    }

    #[test]
    fn the_baseline_reaches_only_as_far_as_the_dots() {
        let rows = draw(&[Some(1.0), Some(2.0), Some(3.0)], false);
        let baseline: String = rows[2 + PLOT_ROWS as usize - 1]
            .chars()
            .skip(SCALE as usize)
            .collect();
        assert_eq!(baseline.trim_end().chars().count(), 2 * STEP as usize + 1);
    }

    #[test]
    fn numbers_the_points_under_the_baseline() {
        let values = vec![Some(1.0); 12];
        let rows = draw(&values, false);
        let numbers = &rows[PLOT_ROWS as usize + 2];
        assert_eq!(
            numbers.split_whitespace().collect::<Vec<_>>(),
            ["1", "5", "10"]
        );
        assert_eq!(numbers.find("10"), Some((SCALE + 9 * STEP) as usize));
    }

    #[test]
    fn the_scale_hugs_the_values_in_round_steps() {
        // Jitter at 20 ms fills the graph rather than sitting in a line at the top.
        assert_eq!(scale(&[Some(20.0), Some(25.0)], 20), (20.0, 2.0, 3));
        // Values near 0 start at 0.
        assert_eq!(scale(&[Some(0.4), None, Some(3.2)], 20), (0.0, 1.0, 4));
        // One value sits inside a scale a fifth of its size, not on its edge.
        assert_eq!(scale(&[Some(240.0)], 6), (200.0, 50.0, 2));
        assert_eq!(scale(&[None], 6), (0.0, 1.0, 1));
        assert_eq!((decimals(5.0), decimals(2.5), decimals(0.25)), (0, 1, 2));
    }
}
