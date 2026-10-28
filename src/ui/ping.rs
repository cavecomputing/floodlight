//! The ping screen: each host's totals up top, a row a ping (the latest that fit, the lost ones
//! in the warning colour), and a graph of each host's round trips along the bottom, side by side.
//! Same look as the speed test's screen.

use crate::ping::pings::{self, Ping, Pings};
use crate::ping::target::Target;
use crate::ui::graph::{PLOT_ROWS, graph};
use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{self, inset, plain};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row};
use ratatui::{DefaultTerminal, Frame};
use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const HINTS: [&str; 2] = ["^r start over", "^c quit"];
/// Graphs side by side in a row, before the next row starts.
const GRAPHS_A_ROW: usize = 3;

pub fn run(targets: &[Target], count: u32, wait: Duration) -> anyhow::Result<()> {
    let ips: Vec<Ipv4Addr> = targets.iter().map(|target| target.ip).collect();
    // Opened before the screen, so a machine that won't let us ping says so in plain text.
    let events = pings::start(&ips, count, wait).map_err(anyhow::Error::msg)?;
    let mut screen = Screen {
        events,
        pings: targets.iter().map(|_| Pings::default()).collect(),
        log: Vec::new(),
        done: false,
        headline: plain::ping_headline(targets, count, wait),
        targets: targets.to_vec(),
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
    /// Every ping in the order it came back: its host, and where it is in that host's pings.
    log: Vec<(usize, usize)>,
    /// Every ping asked for is in.
    done: bool,
    headline: String,
    targets: Vec<Target>,
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
                    Ok(ping) => {
                        let host = &mut self.pings[ping.host].pings;
                        self.log.push((ping.host, host.len()));
                        host.push(ping);
                    }
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
                    // Forget them and start over; dropping the old receiver stops its pings.
                    (KeyCode::Char('r'), true) => {
                        if let Ok(events) = pings::start(ips, self.count, wait) {
                            self.events = events;
                            self.pings = ips.iter().map(|_| Pings::default()).collect();
                            self.log.clear();
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
        let hosts = self.targets.len();
        let graph_rows = hosts.div_ceil(GRAPHS_A_ROW);
        let [_, head, totals, _, table, graphs, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(hosts as u16),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length((PLOT_ROWS + 3) * graph_rows as u16),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let mut lines = vec![self.headline.clone()];
        lines.extend(plain::ping_totals_lines(&self.targets, &self.pings));
        let lines: Vec<Line> = lines
            .into_iter()
            .map(|words| Line::styled(format!("# {words}"), roles.comment()))
            .collect();
        let top = Rect {
            height: head.height + totals.height,
            ..head
        };
        frame.render_widget(Paragraph::new(lines), inset(top));
        self.draw_table(frame, table, roles);

        // Four hosts' graphs go two and two rather than three and one.
        let a_row = hosts.div_ceil(graph_rows.max(1));
        let rows =
            Layout::vertical(vec![Constraint::Length(PLOT_ROWS + 3); graph_rows]).split(graphs);
        for (i, (target, pings)) in self.targets.iter().zip(&self.pings).enumerate() {
            let row = rows[i / a_row];
            let places = Layout::horizontal(vec![Constraint::Fill(1); a_row])
                .spacing(3)
                .split(inset(row));
            let title = match hosts {
                1 => "latency · ms · a gap is a ping lost".to_string(),
                _ => format!("{} · ms", target.label()),
            };
            let round_trips: Vec<Option<f32>> = pings
                .pings
                .iter()
                .map(|ping| ping.reply.as_ref().ok().copied())
                .collect();
            graph(
                frame,
                places[i % a_row],
                &title,
                &round_trips,
                roles.good(),
                roles,
            );
        }
        status_bar::draw(frame, rule, bar, roles, self.state_line(roles), &HINTS);
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

    fn draw_table(&self, frame: &mut Frame, area: Rect, roles: &Roles) {
        // The latest pings, when there are more than the rows can hold.
        let room = area.height.saturating_sub(1) as usize;
        let skipped = self.log.len().saturating_sub(room);
        // A host column only when there is more than one.
        let several = self.targets.len() > 1;
        let rows: Vec<Row<'static>> = self.log[skipped..]
            .iter()
            .map(|&(host, at)| {
                let ping = &self.pings[host].pings[at];
                let [seq, time, latency] = plain::ping_words(ping);
                let latency = match ping.reply {
                    Ok(_) => Span::styled(latency, roles.text()),
                    Err(_) => Span::styled(latency, roles.warning()),
                };
                let mut cells = vec![
                    Cell::from(Span::styled(seq, roles.muted())),
                    Cell::from(Span::styled(time, roles.comment())),
                ];
                if several {
                    let label = self.targets[host].label();
                    cells.push(Cell::from(Span::styled(label, roles.address())));
                }
                cells.push(Cell::from(latency));
                Row::new(cells)
            })
            .collect();
        let mut widths = vec![Constraint::Length(6), Constraint::Length(8)];
        let mut titles = vec!["seq", "time"];
        if several {
            let longest = self.targets.iter().map(|target| target.label().len());
            widths.push(Constraint::Length(longest.max().unwrap_or_default() as u16));
            titles.push("host");
        }
        widths.push(Constraint::Fill(1));
        titles.push("latency");
        ui::comment_table(frame, area, roles, &titles, widths, rows);
    }
}
