//! The LAN speed test's screens, both ends, in the speed test's look. The client's: a row an
//! interval as the data arrives (the transfer under way in the active colour), each transfer's
//! total as a row of its own, and a graph a direction of the intervals along the bottom. The
//! server's: a row a test served (when, who, what was asked, what moved; the one under way
//! in the active colour, its numbers the latest interval's), and the same two graphs.

use crate::lanspeed::runs::{self, Event, Run, Runs, Settings};
use crate::lanspeed::server::{self, Event as ServeEvent, Served, Tests};
use crate::lanspeed::wire::{Direction, Protocol};
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
const SERVE_HINTS: [&str; 2] = ["^r clear", "^c quit"];

pub fn run(settings: Settings, count: u32, wait: Duration) -> anyhow::Result<()> {
    let mut screen = Screen {
        events: runs::start(settings.clone(), count, wait),
        runs: Runs::default(),
        headline: plain::lanspeed_headline(&settings, count),
        settings,
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
    headline: String,
    settings: Settings,
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
                        self.events = runs::start(self.settings.clone(), count, wait);
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
        let directions = self.settings.directions.len() as u16;
        let [_, head, _, table, graphs, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(if directions > 0 { PLOT_ROWS + 3 } else { 0 }),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let mut headline = self.headline.clone();
        if self.runs.runs.len() > 1
            && let Some(medians) = plain::lanspeed_medians(&self.runs)
        {
            headline.push_str(&format!(" · {medians}"));
        }
        frame.render_widget(
            Paragraph::new(Span::styled(format!("# {headline}"), roles.comment())),
            inset(head),
        );
        self.draw_table(frame, table, roles);

        let places = Layout::horizontal(vec![Constraint::Fill(1); directions as usize])
            .spacing(3)
            .split(inset(graphs));
        for (direction, place) in self.settings.directions.iter().zip(places.iter()) {
            let ink = match direction {
                Direction::Upload => roles.address(),
                Direction::Download => roles.good(),
            };
            graph(
                frame,
                *place,
                &format!("{} · Mbps · an interval a point", direction.name()),
                &self.runs.intervals(*direction),
                ink,
                roles,
            );
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
                _ => {
                    let why = self
                        .runs
                        .runs
                        .iter()
                        .rev()
                        .find_map(|run| run.failed.clone())
                        .unwrap_or_default();
                    let words = match started {
                        1 => format!("failed: {why}"),
                        _ => format!("done · {failed} of {started} failed: {why}"),
                    };
                    (roles.warning(), words, roles.warning())
                }
            }
        } else if let Some(next) = self.runs.next {
            let seconds = next.saturating_duration_since(Instant::now()).as_secs() + 1;
            let words = format!("next run in {seconds}s · {started} of {} done", self.asked);
            (roles.comment(), words, roles.muted())
        } else {
            let dot = ui::pulse(self.opened, roles);
            let run = self.runs.runs.last();
            let mut words = match run.and_then(|run| run.phase) {
                Some(direction) => {
                    let seconds = run
                        .map(|run| {
                            run.intervals
                                .iter()
                                .filter(|(way, _)| *way == direction)
                                .map(|(_, report)| report.to)
                                .fold(0.0, f32::max)
                        })
                        .unwrap_or_default();
                    format!(
                        "measuring {} · {seconds:.0} of {} s",
                        direction.name(),
                        self.settings.seconds.as_secs_f32()
                    )
                }
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
        let udp = self.settings.protocol == Protocol::Udp;
        let mut rows: Vec<Row<'static>> = Vec::new();
        for run in &self.runs.runs {
            rows.extend(run_rows(run, roles, udp));
        }
        // The latest rows, when there are more than fit.
        let room = area.height.saturating_sub(1) as usize;
        let skipped = rows.len().saturating_sub(room);
        let rows = rows.split_off(skipped);
        let mut titles = vec!["interval", "way", "bitrate", "transfer"];
        let mut widths = vec![
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Length(12),
            Constraint::Length(10),
        ];
        if udp {
            titles.extend(["jitter", "lost"]);
            widths.extend([Constraint::Length(9), Constraint::Fill(1)]);
        } else {
            widths.pop();
            widths.push(Constraint::Fill(1));
        }
        ui::comment_table(frame, area, roles, &titles, widths, rows);
    }
}

/// A run's rows: an interval each, in the order they came, and each transfer's total after its
/// intervals, in the plain colour so it stands out from them; a failed run ends with why.
fn run_rows(run: &Run, roles: &Roles, udp: bool) -> Vec<Row<'static>> {
    let cells = |words: Vec<String>, style: Style| {
        let mut cells: Vec<Cell> = words
            .into_iter()
            .map(|word| Cell::from(Span::styled(word, style)))
            .collect();
        if udp {
            cells.resize(6, Cell::from(""));
        }
        Row::new(cells)
    };
    let mut rows = Vec::new();
    let mut shown = Vec::new();
    let mut last = None;
    for (direction, report) in &run.intervals {
        // A transfer's total goes after its intervals, before the next transfer's.
        if let Some(before) = last
            && before != *direction
            && let Some(total) = run.total(before)
        {
            shown.push(before);
            rows.push(cells(
                plain::lanspeed_row(before, total, true),
                roles.text(),
            ));
        }
        let style = match run.phase == Some(*direction) {
            true => roles.active(),
            false => roles.muted(),
        };
        rows.push(cells(plain::lanspeed_row(*direction, report, false), style));
        last = Some(*direction);
    }
    for direction in [Direction::Upload, Direction::Download] {
        if !shown.contains(&direction)
            && let Some(total) = run.total(direction)
        {
            rows.push(cells(
                plain::lanspeed_row(direction, total, true),
                roles.text(),
            ));
        }
    }
    if let Some(why) = &run.failed {
        // Why goes in the last column, the one with room.
        let mut words = vec![run.time.clone(), "failed".to_string()];
        words.resize(if udp { 5 } else { 3 }, String::new());
        words.push(why.clone());
        let mut cells: Vec<Cell> = words
            .into_iter()
            .map(|word| Cell::from(Span::styled(word, roles.warning())))
            .collect();
        cells[0] = Cell::from(Span::styled(run.time.clone(), roles.comment()));
        rows.push(Row::new(cells));
    }
    rows
}

/// The serving end's screen: listens on `port` and shows the tests as clients run them.
pub fn serve(port: u16) -> anyhow::Result<()> {
    let mut screen = Serving {
        events: server::serve(port)?,
        tests: Tests::default(),
        port,
        opened: Instant::now(),
    };
    let mut terminal = ratatui::init();
    let outcome = screen.show(&mut terminal);
    ratatui::restore();
    outcome
}

struct Serving {
    events: Receiver<ServeEvent>,
    tests: Tests,
    port: u16,
    opened: Instant,
}

impl Serving {
    fn show(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        loop {
            while let Ok(event) = self.events.try_recv() {
                self.tests.apply(event);
            }
            terminal.draw(|frame| self.draw(frame))?;
            if let Some(key) = ui::pressed()? {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match (key.code, ctrl) {
                    (KeyCode::Char('c'), true) | (KeyCode::Char('q'), true) | (KeyCode::Esc, _) => {
                        return Ok(());
                    }
                    // Forget the tests served, but not one under way.
                    (KeyCode::Char('r'), true) => {
                        let under_way = self.tests.tests.pop().filter(Served::under_way);
                        self.tests.tests = under_way.into_iter().collect();
                    }
                    _ => {}
                }
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let roles = &TERMINAL;
        let [_, head, _, table, graphs, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(PLOT_ROWS + 3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let over = self
            .tests
            .tests
            .iter()
            .filter(|served| !served.under_way())
            .count();
        let headline = plain::lanspeed_serve_headline(self.port, over);
        frame.render_widget(
            Paragraph::new(Span::styled(format!("# {headline}"), roles.comment())),
            inset(head),
        );
        self.draw_table(frame, table, roles);

        let [received, sent] = Layout::horizontal([Constraint::Fill(1); 2])
            .spacing(3)
            .areas(inset(graphs));
        graph(
            frame,
            received,
            "received · Mbps · an interval a point",
            &self.tests.intervals(Direction::Upload),
            roles.address(),
            roles,
        );
        graph(
            frame,
            sent,
            "sent · Mbps · an interval a point",
            &self.tests.intervals(Direction::Download),
            roles.good(),
            roles,
        );
        status_bar::draw(
            frame,
            rule,
            bar,
            roles,
            self.state_line(roles),
            &SERVE_HINTS,
        );
    }

    /// Waiting, or what is moving to or from whom and how far along it is; the last test's
    /// failure stays up until the next client.
    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let (dot, words, tone) = match self.tests.tests.last() {
            Some(served) if served.under_way() => {
                let so_far = served
                    .intervals
                    .last()
                    .map(|report| report.to)
                    .unwrap_or_default();
                let words = format!(
                    "{} {} {} · {so_far:.0} of {} s",
                    match served.test.direction {
                        Direction::Upload => "receiving from",
                        Direction::Download => "sending to",
                    },
                    served.from,
                    served.test.protocol.name(),
                    served.test.seconds.as_secs_f32()
                );
                (ui::pulse(self.opened, roles), words, roles.muted())
            }
            Some(Served {
                failed: Some(why), ..
            }) => (
                roles.warning(),
                format!("failed: {why} · waiting for a client"),
                roles.warning(),
            ),
            Some(served) => (
                roles.good(),
                format!(
                    "{} {} · waiting for a client",
                    served.did(),
                    plain::mbps(served.total.as_ref().map(|total| total.mbps()))
                ),
                roles.muted(),
            ),
            None => (
                ui::pulse(self.opened, roles),
                "waiting for a client".to_string(),
                roles.muted(),
            ),
        };
        Line::from(vec![Span::styled("● ", dot), Span::styled(words, tone)])
    }

    fn draw_table(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let udp = self
            .tests
            .tests
            .iter()
            .any(|served| served.test.protocol == Protocol::Udp);
        // The latest tests, when there are more than fit.
        let room = area.height.saturating_sub(1) as usize;
        let skipped = self.tests.tests.len().saturating_sub(room);
        let rows: Vec<Row<'static>> = self.tests.tests[skipped..]
            .iter()
            .map(|served| served_row(served, roles, udp))
            .collect();
        let mut titles = vec!["time", "client", "test", "asked", "rate", "bitrate"];
        let mut widths = vec![
            Constraint::Length(8),
            Constraint::Length(15),
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Length(9),
            Constraint::Length(12),
        ];
        if udp {
            titles.extend(["jitter", "lost"]);
            widths.extend([Constraint::Length(8), Constraint::Fill(1)]);
        } else {
            // TCP asks for no rate: the column goes.
            titles.remove(4);
            widths.remove(4);
            widths.pop();
            widths.push(Constraint::Fill(1));
        }
        ui::comment_table(frame, area, roles, &titles, widths, rows);
    }
}

/// A served test's row: the one under way in the active colour, over in the plain colour, and
/// a failed one in the warning colour with why in the bitrate's place, spanning what's left.
fn served_row(served: &Served, roles: &Roles, udp: bool) -> Row<'static> {
    let mut words = plain::lanspeed_served_row(served);
    if !udp {
        words.remove(4);
    }
    let style = match (served.under_way(), &served.failed) {
        (true, _) => roles.active(),
        (false, Some(_)) => roles.warning(),
        (false, None) => roles.text(),
    };
    let mut cells: Vec<Cell> = words
        .into_iter()
        .map(|word| Cell::from(Span::styled(word, style)))
        .collect();
    cells[0] = Cell::from(Span::styled(served.time.clone(), roles.comment()));
    Row::new(cells)
}
