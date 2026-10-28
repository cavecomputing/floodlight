//! The trace screen: what the trace has found up top, then a row a hop on the way, the share each
//! lost in the warning colour once it lost any. Same look as the ping screen.

use crate::ping::target::Target;
use crate::ping::trace::{self, Event, Trace};
use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{self, inset, plain};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row};
use ratatui::{DefaultTerminal, Frame};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const HINTS: [&str; 2] = ["^r start over", "^c quit"];

pub fn run(target: &Target, count: u32, wait: Duration) -> anyhow::Result<()> {
    // Opened before the screen, so a machine that won't let us ping says so in plain text.
    let events = trace::start(target.ip, count, wait).map_err(anyhow::Error::msg)?;
    let mut screen = Screen {
        events,
        trace: Trace::new(target.ip),
        done: false,
        headline: plain::trace_headline(target, count, wait),
        opened: Instant::now(),
    };
    let mut terminal = ratatui::init();
    let outcome = screen.show(&mut terminal, count, wait);
    ratatui::restore();
    outcome
}

struct Screen {
    events: Receiver<Event>,
    trace: Trace,
    /// Every round asked for is in.
    done: bool,
    headline: String,
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
            loop {
                match self.events.try_recv() {
                    Ok(event) => self.trace.add(event),
                    Err(TryRecvError::Disconnected) => break self.done = true,
                    Err(TryRecvError::Empty) => break,
                }
            }
            terminal.draw(|frame| self.draw(frame))?;
            if let Some(key) = ui::pressed()? {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match (key.code, ctrl) {
                    (KeyCode::Char('c'), true) | (KeyCode::Char('q'), true) | (KeyCode::Esc, _) => {
                        return Ok(());
                    }
                    // Forget it and start over; dropping the old receiver stops its echoes.
                    (KeyCode::Char('r'), true) => {
                        if let Ok(events) = trace::start(self.trace.ip, count, wait) {
                            self.events = events;
                            self.trace = Trace::new(self.trace.ip);
                            self.done = false;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let roles = &TERMINAL;
        let [_, top, _, table, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let lines: Vec<Line> = [self.headline.clone(), plain::trace_verdict(&self.trace)]
            .into_iter()
            .map(|words| Line::styled(format!("# {words}"), roles.comment()))
            .collect();
        frame.render_widget(Paragraph::new(lines), inset(top));
        self.draw_table(frame, table, roles);
        status_bar::draw(frame, rule, bar, roles, self.state_line(roles), &HINTS);
    }

    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let rounds = self.trace.rounds();
        let plural = if rounds == 1 { "" } else { "s" };
        let (dot, words) = match self.done {
            true => (roles.good(), format!("done · {rounds} round{plural}")),
            false => (
                ui::pulse(self.opened, roles),
                format!("tracing · round {}", rounds.max(1)),
            ),
        };
        Line::from(vec![
            Span::styled("● ", dot),
            Span::styled(words, roles.muted()),
        ])
    }

    fn draw_table(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let rows: Vec<Row<'static>> = self
            .trace
            .path()
            .iter()
            .enumerate()
            .map(|(at, hop)| {
                let [hop_number, address, name, loss, last, min, average, max] =
                    plain::trace_words(at, hop);
                let loss_tone = match hop.pings.lost() {
                    0 => roles.muted(),
                    _ => roles.warning(),
                };
                let address_tone = match (hop.routers.is_empty(), hop.pings.pings.is_empty()) {
                    (true, true) => roles.comment(),
                    (true, false) => roles.warning(),
                    (false, _) => roles.address(),
                };
                Row::new([
                    Cell::from(Span::styled(hop_number, roles.muted())),
                    Cell::from(Span::styled(address, address_tone)),
                    Cell::from(Span::styled(name, roles.text())),
                    Cell::from(Span::styled(loss, loss_tone)),
                    Cell::from(Span::styled(last, roles.text())),
                    Cell::from(Span::styled(min, roles.muted())),
                    Cell::from(Span::styled(average, roles.muted())),
                    Cell::from(Span::styled(max, roles.muted())),
                ])
            })
            .collect();
        let widths = vec![
            Constraint::Length(3),
            Constraint::Length(19),
            Constraint::Fill(1),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
        ];
        ui::comment_table(frame, area, roles, &plain::TRACE_TITLES, widths, rows);
    }
}
