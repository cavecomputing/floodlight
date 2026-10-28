//! The speed test's screen: a row a run, filling in as each is measured, the one being measured
//! in the active colour, and with `--count`, a graph each of download, upload and latency across
//! the runs. Same look as the other screens, without the prompt or a moving band.

use crate::speedtest::runs::{self, Event, Phase, Run, Runs};
use crate::ui::graph::{PLOT_ROWS, graph};
use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{self, inset, plain};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row};
use ratatui::{DefaultTerminal, Frame};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const HINTS: [&str; 2] = ["^r test again", "^c quit"];

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
            if let Some(key) = ui::pressed()? {
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
            let dot = ui::pulse(self.opened, roles);
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
        ui::comment_table(frame, area, roles, &titles, widths.to_vec(), rows);
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
