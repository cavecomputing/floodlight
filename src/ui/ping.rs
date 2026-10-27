//! The ping screen: the totals up top, a row a ping (the latest that fit, the lost ones in the
//! warning colour), and a graph of the round trips along the bottom. Same look as the speed
//! test's screen.

use crate::ping::pings::{self, Ping, Pings};
use crate::ping::target::Target;
use crate::ui::speedtest::{PLOT_ROWS, graph};
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
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(50);
const PULSE: Duration = Duration::from_millis(600);
const HINTS: [&str; 2] = ["^r start over", "^c quit"];

pub fn run(target: &Target, count: u32, wait: Duration) -> anyhow::Result<()> {
    // Opened before the screen, so a machine that won't let us ping says so in plain text.
    let events = pings::start(target.ip, count, wait).map_err(anyhow::Error::msg)?;
    let mut screen = Screen {
        events,
        pings: Pings::default(),
        done: false,
        headline: plain::ping_headline(target, count, wait),
        count,
        opened: Instant::now(),
    };
    let mut terminal = ratatui::init();
    let outcome = screen.show(&mut terminal, target, wait);
    ratatui::restore();
    outcome
}

struct Screen {
    events: Receiver<Ping>,
    pings: Pings,
    /// Every ping asked for is in.
    done: bool,
    headline: String,
    count: u32,
    opened: Instant,
}

impl Screen {
    fn show(
        &mut self,
        terminal: &mut DefaultTerminal,
        target: &Target,
        wait: Duration,
    ) -> anyhow::Result<()> {
        loop {
            loop {
                match self.events.try_recv() {
                    Ok(ping) => self.pings.pings.push(ping),
                    Err(TryRecvError::Disconnected) => break self.done = true,
                    Err(TryRecvError::Empty) => break,
                }
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
                    // Forget them and start over; dropping the old receiver stops its pings.
                    (KeyCode::Char('r'), true) => {
                        if let Ok(events) = pings::start(target.ip, self.count, wait) {
                            self.events = events;
                            self.pings = Pings::default();
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
        frame.render_widget(
            Block::new().style(Style::new().bg(roles.bg_surface).fg(roles.fg)),
            frame.area(),
        );
        let [_, head, totals, _, table, graphs, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(PLOT_ROWS + 3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        for (area, words) in [
            (head, self.headline.clone()),
            (totals, plain::ping_totals(&self.pings)),
        ] {
            frame.render_widget(
                Paragraph::new(Span::styled(format!("# {words}"), roles.comment())),
                inset(area),
            );
        }
        self.draw_table(frame, table, roles);
        let round_trips: Vec<Option<f32>> = self
            .pings
            .pings
            .iter()
            .map(|ping| ping.reply.as_ref().ok().copied())
            .collect();
        graph(
            frame,
            inset(graphs),
            "latency · ms · a gap is a ping lost",
            &round_trips,
            roles.good(),
            roles,
        );
        status_bar::draw(frame, rule, bar, roles, self.state_line(roles), &HINTS);
    }

    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let sent = self.pings.pings.len();
        let lost = self.pings.lost();
        let (dot, words, tone) = if self.done {
            match lost {
                0 => (roles.good(), format!("done · {sent} back"), roles.muted()),
                _ => (
                    roles.warning(),
                    format!("done · {lost} of {sent} lost"),
                    roles.warning(),
                ),
            }
        } else {
            let lit = (self.opened.elapsed().as_millis() / PULSE.as_millis()).is_multiple_of(2);
            let dot = if lit { roles.active() } else { roles.comment() };
            let words = match self.count {
                0 => format!("pinging · {sent} sent"),
                count => format!("pinging · {sent} of {count}"),
            };
            (dot, words, roles.muted())
        };
        Line::from(vec![Span::styled("● ", dot), Span::styled(words, tone)])
    }

    fn draw_table(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        // The latest pings, when there are more than the rows can hold.
        let room = area.height.saturating_sub(1) as usize;
        let skipped = self.pings.pings.len().saturating_sub(room);
        let rows: Vec<Row<'static>> = self.pings.pings[skipped..]
            .iter()
            .map(|ping| {
                let [seq, time, latency] = plain::ping_words(ping);
                let latency = match ping.reply {
                    Ok(_) => Span::styled(latency, roles.text()),
                    Err(_) => Span::styled(latency, roles.warning()),
                };
                Row::new(vec![
                    Cell::from(Span::styled(seq, roles.muted())),
                    Cell::from(Span::styled(time, roles.comment())),
                    Cell::from(latency),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Fill(1),
        ];
        let header = Row::new(
            ["seq", "time", "latency"]
                .map(|title| Cell::from(Span::styled(title, roles.comment()))),
        );
        let table = Table::new(rows, widths).header(header).column_spacing(2);
        // The rows start two columns in from the `#` that comments out the header, like the
        // other screens' tables.
        let rows = Rect {
            x: area.x + INSET + 2,
            width: area.width.saturating_sub(INSET + 2),
            ..area
        };
        frame.render_widget(table, rows);
        if area.height > 0 {
            frame.buffer_mut()[(area.x + INSET, area.y)]
                .set_char('#')
                .set_style(roles.comment());
        }
    }
}
