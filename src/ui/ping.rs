//! The ping screen: a row a host up top with its average and totals, marked with a dot in its
//! own colour, then a graph filling the rest with the round trips of the host picked with the
//! arrows, in that colour. Same look as the speed test's screen.

use crate::ping::pings::{self, Ping, Pings};
use crate::ping::target::Target;
use crate::ui::graph::graph;
use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{self, inset, plain};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row};
use ratatui::{DefaultTerminal, Frame};
use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const HINTS: [&str; 2] = ["^r start over", "^c quit"];
/// With several hosts, the arrows pick the one on the graph.
const HINTS_SEVERAL: [&str; 3] = ["↑↓ pick a host", "^r start over", "^c quit"];

pub fn run(targets: &[Target], count: u32, wait: Duration) -> anyhow::Result<()> {
    let ips: Vec<Ipv4Addr> = targets.iter().map(|target| target.ip).collect();
    // Opened before the screen, so a machine that won't let us ping says so in plain text.
    let events = pings::start(&ips, count, wait).map_err(anyhow::Error::msg)?;
    let mut screen = Screen {
        events,
        pings: targets.iter().map(|_| Pings::default()).collect(),
        done: false,
        headline: plain::ping_headline(targets, count, wait),
        targets: targets.to_vec(),
        selected: 0,
        count,
        opened: Instant::now(),
    };
    let mut terminal = ratatui::init();
    let outcome = screen.show(&mut terminal, &ips, wait);
    ratatui::restore();
    outcome
}

struct Screen {
    events: Receiver<Ping>,
    /// Each host's pings.
    pings: Vec<Pings>,
    /// Every ping asked for is in.
    done: bool,
    headline: String,
    targets: Vec<Target>,
    /// The host whose round trips the graph shows, picked with the arrows.
    selected: usize,
    count: u32,
    opened: Instant,
}

impl Screen {
    fn show(
        &mut self,
        terminal: &mut DefaultTerminal,
        ips: &[Ipv4Addr],
        wait: Duration,
    ) -> anyhow::Result<()> {
        loop {
            loop {
                match self.events.try_recv() {
                    Ok(ping) => self.pings[ping.host].pings.push(ping),
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
                    (KeyCode::Up, _) => self.selected = self.selected.saturating_sub(1),
                    (KeyCode::Down, _) if self.selected + 1 < self.targets.len() => {
                        self.selected += 1;
                    }
                    // Forget them and start over; dropping the old receiver stops its pings.
                    (KeyCode::Char('r'), true) => {
                        if let Ok(events) = pings::start(ips, self.count, wait) {
                            self.events = events;
                            self.pings = ips.iter().map(|_| Pings::default()).collect();
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
        let hosts = self.targets.len() as u16;
        // The graph takes every row the hosts leave, so it is as tall as the window allows.
        let [_, head, _, rundown, graph_area, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(hosts + 1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("# {}", self.headline),
                roles.comment(),
            )),
            inset(head),
        );
        self.draw_rundown(frame, rundown, roles);

        let i = self.selected;
        let round_trips: Vec<Option<f32>> = self.pings[i]
            .pings
            .iter()
            .map(|ping| ping.reply.as_ref().ok().copied())
            .collect();
        let title = match self.targets.len() {
            1 => "latency · ms · × a ping lost".to_string(),
            _ => format!("{} · latency · ms · × a ping lost", self.targets[i].label()),
        };
        graph(
            frame,
            inset(graph_area),
            &title,
            &round_trips,
            (self.count > 0).then_some(self.count as usize),
            roles.host(i),
            true,
            roles,
        );
        let hints: &[&str] = if self.targets.len() > 1 {
            &HINTS_SEVERAL
        } else {
            &HINTS
        };
        status_bar::draw(frame, rule, bar, roles, self.state_line(roles), hints);
    }

    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let sent: usize = self.pings.iter().map(|pings| pings.pings.len()).sum();
        let lost: usize = self.pings.iter().map(Pings::lost).sum();
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
            let dot = ui::pulse(self.opened, roles);
            let words = match self.count {
                0 => format!("pinging · {sent} sent"),
                count => format!("pinging · {sent} of {}", count as usize * self.pings.len()),
            };
            (dot, words, roles.muted())
        };
        Line::from(vec![Span::styled("● ", dot), Span::styled(words, tone)])
    }

    /// A row a host: its dot in its colour and its name, then the average and the rest.
    fn draw_rundown(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let several = self.targets.len() > 1;
        let ms = |value: Option<f32>| value.map_or("—".to_string(), |ms| format!("{ms:.2} ms"));
        let rows: Vec<Row<'static>> = self
            .targets
            .iter()
            .zip(&self.pings)
            .enumerate()
            .map(|(i, (target, pings))| {
                // With several hosts, the one on the graph is marked like the prompt: a band
                // vanishes in a theme whose bright black is its background (Solarized Dark).
                let picked = self.selected == i;
                let marker = match (several, picked) {
                    (false, _) => Span::raw(""),
                    (true, true) => Span::styled("> ", roles.active()),
                    (true, false) => Span::raw("  "),
                };
                let name = match several && picked {
                    true => roles.text().add_modifier(Modifier::BOLD),
                    false => roles.text(),
                };
                let host = Line::from(vec![
                    marker,
                    Span::styled("● ", roles.host(i)),
                    Span::styled(target.label(), name),
                ]);
                let lost = format!("{} · {:.1}%", pings.lost(), pings.loss());
                let lost = match pings.lost() {
                    0 => Span::styled(lost, roles.muted()),
                    _ => Span::styled(lost, roles.warning()),
                };
                Row::new(vec![
                    Cell::from(host),
                    Cell::from(Span::styled(ms(pings.average()), roles.text())),
                    Cell::from(Span::styled(ms(pings.min()), roles.muted())),
                    Cell::from(Span::styled(ms(pings.max()), roles.muted())),
                    Cell::from(Span::styled(ms(pings.jitter()), roles.muted())),
                    Cell::from(Span::styled(pings.pings.len().to_string(), roles.muted())),
                    Cell::from(lost),
                ])
            })
            .collect();
        let longest = self.targets.iter().map(|target| target.label().len());
        let mut widths = vec![Constraint::Length(
            longest.max().unwrap_or_default() as u16 + if several { 4 } else { 2 },
        )];
        widths.extend([Constraint::Length(10); 4]);
        widths.extend([Constraint::Length(6), Constraint::Fill(1)]);
        let titles = ["host", "avg", "min", "max", "jitter", "sent", "lost"];
        ui::comment_table(frame, area, roles, &titles, widths, rows);
    }
}
