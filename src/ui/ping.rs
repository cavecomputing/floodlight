//! The ping screen: a row a host up top with its average and totals, marked with a dot in its
//! own colour, a row a ping (the latest that fit, the lost ones in the warning colour), and one
//! graph along the bottom with every host's round trips on it in those colours. Same look as the
//! speed test's screen.

use crate::ping::pings::{self, Ping, Pings};
use crate::ping::target::Target;
use crate::ui::graph::shared_graph;
use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{self, inset, plain};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row};
use ratatui::{DefaultTerminal, Frame};
use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const HINTS: [&str; 2] = ["^r start over", "^c quit"];
/// With several hosts, tab picks one to see alone on the graph.
const HINTS_SEVERAL: [&str; 3] = ["tab one host", "^r start over", "^c quit"];
/// Rows the graph plots in: taller than the other screens', since every host shares it.
const PLOT_ROWS: u16 = 10;

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
        focus: None,
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
    /// The one host shown alone on the graph (tab), or None for all of them.
    focus: Option<usize>,
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
                    // All the hosts, then each alone, then all again.
                    (KeyCode::Tab, _) if self.targets.len() > 1 => {
                        self.focus = match self.focus {
                            None => Some(0),
                            Some(i) if i + 1 < self.targets.len() => Some(i + 1),
                            Some(_) => None,
                        };
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
        let hosts = self.targets.len() as u16;
        let [_, head, _, rundown, _, table, graph, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(hosts + 1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(PLOT_ROWS + 3),
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
        self.draw_table(frame, table, roles);

        let round_trips: Vec<Vec<Option<f32>>> = self
            .pings
            .iter()
            .map(|pings| {
                let replies = pings.pings.iter();
                replies
                    .map(|ping| ping.reply.as_ref().ok().copied())
                    .collect()
            })
            .collect();
        let series: Vec<(&[Option<f32>], Style)> = round_trips
            .iter()
            .enumerate()
            .filter(|(i, _)| self.focus.is_none_or(|focus| focus == *i))
            .map(|(i, values)| (values.as_slice(), roles.host(i)))
            .collect();
        let title = match self.focus {
            Some(i) => format!(
                "{} · latency · ms · a gap is a ping lost",
                self.targets[i].label()
            ),
            None => "latency · ms · a gap is a ping lost".to_string(),
        };
        shared_graph(frame, inset(graph), &title, &series, roles);
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
        let ms = |value: Option<f32>| value.map_or("—".to_string(), |ms| format!("{ms:.2} ms"));
        let rows: Vec<Row<'static>> = self
            .targets
            .iter()
            .zip(&self.pings)
            .enumerate()
            .map(|(i, (target, pings))| {
                let host = Line::from(vec![
                    Span::styled("● ", roles.host(i)),
                    Span::styled(target.label(), roles.text()),
                ]);
                let lost = format!("{} · {:.1}%", pings.lost(), pings.loss());
                let lost = match pings.lost() {
                    0 => Span::styled(lost, roles.muted()),
                    _ => Span::styled(lost, roles.warning()),
                };
                let row = Row::new(vec![
                    Cell::from(host),
                    Cell::from(Span::styled(ms(pings.average()), roles.text())),
                    Cell::from(Span::styled(ms(pings.min()), roles.muted())),
                    Cell::from(Span::styled(ms(pings.max()), roles.muted())),
                    Cell::from(Span::styled(ms(pings.jitter()), roles.muted())),
                    Cell::from(Span::styled(pings.pings.len().to_string(), roles.muted())),
                    Cell::from(lost),
                ]);
                // The host on the graph alone sits on the band, like the active row elsewhere.
                match self.focus == Some(i) {
                    true => row.style(roles.band()),
                    false => row,
                }
            })
            .collect();
        let longest = self.targets.iter().map(|target| target.label().len());
        let mut widths = vec![Constraint::Length(
            longest.max().unwrap_or_default() as u16 + 2,
        )];
        widths.extend([Constraint::Length(10); 4]);
        widths.extend([Constraint::Length(6), Constraint::Fill(1)]);
        let titles = ["host", "avg", "min", "max", "jitter", "sent", "lost"];
        ui::comment_table(frame, area, roles, &titles, widths, rows);
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
                    cells.push(Cell::from(Span::styled(label, roles.host(host))));
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
