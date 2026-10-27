//! The speed test's screen: a row a run, filling in as each is measured, the one being measured
//! in the active colour, and with `--count`, a graph each of download, upload and latency across
//! the runs. Same look as the other screens, without the prompt or a moving band.

use crate::speedtest::runs::{self, Event, Phase, Run, Runs};
use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{INSET, inset, plain};
use ratatui::crossterm::event::{
    self as input, Event as Input, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table};
use ratatui::{DefaultTerminal, Frame};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(50);
const PULSE: Duration = Duration::from_millis(600);
const HINTS: [&str; 2] = ["^r test again", "^c quit"];
/// Rows a graph plots in, under its title.
pub const PLOT_ROWS: u16 = 6;
/// Columns a graph's scale takes, left of its points.
const SCALE: u16 = 7;

pub fn run(count: u32, wait: Duration) -> anyhow::Result<()> {
    let mut screen = Screen {
        events: runs::start(count, wait),
        runs: Runs::default(),
        asked: count,
        opened: Instant::now(),
    };
    let mut terminal = ratatui::init();
    let outcome = screen.show(&mut terminal, count, wait);
    ratatui::restore();
    outcome
}

struct Screen {
    events: Receiver<Event>,
    runs: Runs,
    /// Runs asked for so far, ^r included.
    asked: u32,
    opened: Instant,
}

impl Screen {
    fn show(
        &mut self,
        terminal: &mut DefaultTerminal,
        count: u32,
        wait: Duration,
    ) -> anyhow::Result<()> {
        loop {
            while let Ok(event) = self.events.try_recv() {
                self.runs.apply(event);
            }
            terminal.draw(|frame| self.draw(frame))?;
            if input::poll(TICK)?
                && let Input::Key(key) = input::read()?
                && key.kind != KeyEventKind::Release
            {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match (key.code, ctrl) {
                    (KeyCode::Char('c'), true) | (KeyCode::Char('q'), true) | (KeyCode::Esc, _) => {
                        return Ok(());
                    }
                    // Another round of the same runs, under the ones already measured.
                    (KeyCode::Char('r'), true) if self.runs.done => {
                        self.events = runs::start(count, wait);
                        self.runs.done = false;
                        self.asked += count;
                    }
                    _ => {}
                }
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let roles = &TERMINAL;
        frame.render_widget(
            Block::new().style(Style::new().bg(roles.bg_surface).fg(roles.fg)),
            frame.area(),
        );
        let [_, head, _, table, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("# {}", headline(&self.runs)),
                roles.comment(),
            )),
            inset(head),
        );
        if self.asked > 1 {
            let [table, graphs] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(PLOT_ROWS + 3)])
                    .areas(table);
            self.draw_table(frame, table, roles);
            self.draw_graphs(frame, graphs, roles);
        } else {
            self.draw_table(frame, table, roles);
        }
        status_bar::draw(frame, rule, bar, roles, self.state_line(roles), &HINTS);
    }

    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let started = self.runs.runs.len();
        let failed = self
            .runs
            .runs
            .iter()
            .filter(|run| run.failed.is_some())
            .count();
        let (dot, words, tone) = if self.runs.done {
            let runs = match started {
                1 => "1 run".to_string(),
                _ => format!("{started} runs"),
            };
            match failed {
                0 => (roles.good(), format!("done · {runs}"), roles.muted()),
                _ => (
                    roles.warning(),
                    format!("done · {failed} of {started} failed"),
                    roles.warning(),
                ),
            }
        } else if let Some(next) = self.runs.next {
            let seconds = next.saturating_duration_since(Instant::now()).as_secs() + 1;
            let words = format!("next run in {seconds}s · {started} of {} done", self.asked);
            (roles.comment(), words, roles.muted())
        } else {
            let lit = (self.opened.elapsed().as_millis() / PULSE.as_millis()).is_multiple_of(2);
            let dot = if lit { roles.active() } else { roles.comment() };
            let phase = self.runs.runs.last().and_then(|run| run.phase);
            let mut words = match phase {
                Some(phase) => format!("measuring {}", phase.name()),
                None => "connecting".to_string(),
            };
            if self.asked > 1 {
                words.push_str(&format!(" · run {} of {}", started.max(1), self.asked));
            }
            (dot, words, roles.muted())
        };
        Line::from(vec![Span::styled("● ", dot), Span::styled(words, tone)])
    }

    fn draw_table(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        // The latest runs, when there are more than the rows can hold.
        let room = area.height.saturating_sub(1) as usize;
        let skipped = self.runs.runs.len().saturating_sub(room);
        let rows: Vec<Row<'static>> = self.runs.runs[skipped..]
            .iter()
            .map(|run| row(run, roles))
            .collect();
        let titles = ["time", "latency", "jitter", "download", "upload", "server"];
        let widths = [
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Fill(1),
        ];
        let header = Row::new(titles.map(|title| Cell::from(Span::styled(title, roles.comment()))));
        let table = Table::new(rows, widths).header(header).column_spacing(2);
        // The rows start two columns in from the `#` that comments out the header, like the
        // other screens' tables.
        let rows = Rect {
            x: area.x + INSET + 2,
            width: area.width.saturating_sub(INSET + 2),
            ..area
        };
        frame.render_widget(table, rows);
        if area.height > 0 && area.width > INSET {
            frame.buffer_mut()[(area.x + INSET, area.y)]
                .set_char('#')
                .set_style(roles.comment());
        }
    }

    /// Download, upload and latency side by side, each a graph of its runs.
    fn draw_graphs(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let [download, upload, latency] = Layout::horizontal([Constraint::Fill(1); 3])
            .spacing(3)
            .areas(inset(area));
        let each = |result: fn(&Run) -> Option<f32>| -> Vec<Option<f32>> {
            self.runs.runs.iter().map(result).collect()
        };
        graph(
            frame,
            download,
            "download · Mbps",
            &each(Run::download),
            roles.good(),
            roles,
        );
        graph(
            frame,
            upload,
            "upload · Mbps",
            &each(Run::upload),
            roles.address(),
            roles,
        );
        graph(
            frame,
            latency,
            "latency · ms",
            &each(Run::latency),
            roles.text(),
            roles,
        );
    }
}

/// One result across the runs: a `●` a run, two columns apart, as high as its value on a round
/// scale, over a `─` baseline. A run that didn't measure it leaves a gap; the latest runs win
/// when there are more than fit. The ping screen draws its graph the same way.
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
    buffer.set_string(
        area.x + SCALE,
        last,
        baseline,
        Style::new().fg(roles.border),
    );
    for (i, value) in shown.iter().enumerate() {
        if let Some(value) = value {
            let steps = (value / top * (PLOT_ROWS - 1) as f32).round() as u16;
            let y = last - steps.min(PLOT_ROWS - 1);
            buffer.set_string(area.x + SCALE + 2 * i as u16, y, "●", ink);
        }
    }
}

/// `cloudflare LAX · Los Angeles · Example Fiber`, then the medians once two runs are in.
fn headline(runs: &Runs) -> String {
    let mut words = match runs.server() {
        Some(server) => {
            let mut words = format!("cloudflare {}", server.place());
            if let Some(isp) = &server.isp {
                words.push_str(&format!(" · {isp}"));
            }
            words
        }
        None => "speed test against cloudflare".to_string(),
    };
    let measured = runs
        .runs
        .iter()
        .filter(|run| run.upload().is_some())
        .count();
    if measured > 1
        && let (Some(down), Some(up)) = (runs.median(Run::download), runs.median(Run::upload))
    {
        words.push_str(&format!(" · median {down:.1} Mbps down · {up:.1} Mbps up"));
    }
    words
}

/// One run's row. The result being measured shows its value so far in the active colour.
fn row(run: &Run, roles: &Roles) -> Row<'static> {
    let [time, latency, jitter, download, upload, server] = plain::run_words(run);
    let cell = |words: String, phase: Phase, measured: Style| {
        let measuring = run.phase == Some(phase);
        let style = match () {
            _ if measuring => roles.active(),
            _ if words == "—" => roles.comment(),
            _ => measured,
        };
        let words = if measuring && words == "—" {
            "measuring".to_string()
        } else {
            words
        };
        Cell::from(Span::styled(words, style))
    };
    let server = match run.failed {
        Some(_) => Span::styled(server, roles.warning()),
        None => Span::styled(server, roles.muted()),
    };
    Row::new(vec![
        Cell::from(Span::styled(time, roles.comment())),
        cell(latency, Phase::Latency, roles.muted()),
        cell(jitter, Phase::Latency, roles.muted()),
        cell(download, Phase::Download, roles.text()),
        cell(upload, Phase::Upload, roles.text()),
        Cell::from(server),
    ])
}
