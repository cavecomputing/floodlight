//! The wireless screen: every network heard, its channel and width, who shares or overlaps it,
//! and a strip of its band so the rows stacked together draw the overlap. Same look as the sweep's
//! screen, without the prompt. Live, it scans again as soon as a scan is in, keeps every access
//! point it has heard in a steady order, and shows how each one's signal moves.

use crate::ui::status_bar;
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{INSET, inset};
use crate::wifi::channels::{self, Band, Crowd};
use crate::wifi::survey::{self, Network};
use crate::wifi::trend::{self, Reading, Trend};
use ratatui::crossterm::event::{
    self as input, Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, HighlightSpacing, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

const TICK: Duration = Duration::from_millis(50);
const PULSE: Duration = Duration::from_millis(600);
/// The pause between one live scan answering and the next one starting.
const LIVE_GAP: Duration = Duration::from_secs(1);
const HINTS: [&str; 2] = ["^r scan again", "^c quit"];

pub fn run(runtime: &Runtime, live: bool) -> anyhow::Result<()> {
    let mut screen = Screen {
        runtime,
        live,
        survey: start(runtime),
        scanning: true,
        next_scan: None,
        heard: None,
        trend: Trend::default(),
        selected: None,
        table: TableState::default(),
        opened: Instant::now(),
        quit: false,
    };
    let mut terminal = ratatui::init();
    let outcome = screen.show(&mut terminal);
    ratatui::restore();
    outcome
}

/// Starts a survey on the runtime; the answer arrives on the receiver.
fn start(runtime: &Runtime) -> oneshot::Receiver<(Result<Vec<Network>, String>, Duration)> {
    let (tx, rx) = oneshot::channel();
    runtime.spawn(async move {
        let started = Instant::now();
        let heard = survey::survey().await;
        let _ = tx.send((heard, started.elapsed()));
    });
    rx
}

/// `14 networks · quietest 2.4 GHz 11 · 5 GHz 149`, with 6 GHz only once something is on it.
pub fn headline(networks: &[Network]) -> String {
    let mut quietest = Vec::new();
    for band in [Band::Ghz2, Band::Ghz5, Band::Ghz6] {
        let heard = networks.iter().any(|n| Band::of(n.freq_mhz) == band);
        if band != Band::Ghz6 || heard {
            let channel = channels::quietest(networks, band);
            quietest.push(format!("{} {channel}", band.name()));
        }
    }
    let count = networks.len();
    format!("{count} networks · quietest {}", quietest.join(" · "))
}

/// `20 MHz`, or `—` when the OS didn't say.
pub fn width(network: &Network) -> String {
    match network.width_mhz {
        Some(mhz) => format!("{mhz} MHz"),
        None => "—".to_string(),
    }
}

struct Screen<'a> {
    runtime: &'a Runtime,
    live: bool,
    survey: oneshot::Receiver<(Result<Vec<Network>, String>, Duration)>,
    scanning: bool,
    /// When the next live scan starts.
    next_scan: Option<Instant>,
    /// The last scan's answer, kept on screen while the next one is out. None before the first.
    heard: Option<(Result<Vec<Network>, String>, Duration)>,
    trend: Trend,
    /// The row the band is on, by access point, so it follows it as the rows re-sort.
    selected: Option<String>,
    table: TableState,
    opened: Instant,
    quit: bool,
}

impl Screen<'_> {
    fn show(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.quit {
            if let Ok(heard) = self.survey.try_recv() {
                if let (Ok(networks), _) = &heard {
                    self.trend.record(networks);
                }
                self.heard = Some(heard);
                self.scanning = false;
                if self.live {
                    self.next_scan = Some(Instant::now() + LIVE_GAP);
                }
            }
            if self.next_scan.is_some_and(|at| at <= Instant::now()) {
                self.scan_again();
            }
            terminal.draw(|frame| self.draw(frame))?;
            if input::poll(TICK)?
                && let Input::Key(key) = input::read()?
                && key.kind != KeyEventKind::Release
            {
                self.press(key);
            }
        }
        Ok(())
    }

    fn press(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('c'), true) | (KeyCode::Char('q'), true) | (KeyCode::Esc, _) => {
                self.quit = true
            }
            (KeyCode::Char('r'), true) => self.scan_again(),
            (KeyCode::Up, _) | (KeyCode::Char('p'), true) => self.step(-1),
            (KeyCode::Down, _) | (KeyCode::Char('n'), true) => self.step(1),
            (KeyCode::PageUp, _) => self.step(-10),
            (KeyCode::PageDown, _) => self.step(10),
            (KeyCode::Home, _) => self.step(i32::MIN / 2),
            (KeyCode::End, _) => self.step(i32::MAX / 2),
            _ => {}
        }
    }

    /// The networks in the last scan.
    fn heard(&self) -> &[Network] {
        match &self.heard {
            Some((Ok(networks), _)) => networks,
            _ => &[],
        }
    }

    /// The rows: live, everything heard so far with its reading; otherwise the last scan.
    fn rows(&self) -> Vec<(Network, Option<Reading>)> {
        if self.live {
            return self
                .trend
                .networks()
                .into_iter()
                .map(|(network, reading)| (network, Some(reading)))
                .collect();
        }
        self.heard()
            .iter()
            .map(|network| (network.clone(), None))
            .collect()
    }

    /// The band's row among the rows.
    fn index(&self, rows: &[(Network, Option<Reading>)]) -> usize {
        rows.iter()
            .position(|(network, _)| Some(trend::key(network)) == self.selected)
            .unwrap_or(0)
    }

    fn step(&mut self, by: i32) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let last = rows.len() as i32 - 1;
        let to = (self.index(&rows) as i32).saturating_add(by).clamp(0, last) as usize;
        self.selected = Some(trend::key(&rows[to].0));
    }

    fn scan_again(&mut self) {
        if self.scanning {
            return;
        }
        self.survey = start(self.runtime);
        self.scanning = true;
        self.next_scan = None;
        self.opened = Instant::now();
    }

    fn draw(&mut self, frame: &mut Frame) {
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

        let mut words = match &self.heard {
            Some((Ok(networks), _)) => headline(networks),
            Some((Err(_), _)) => "nothing to show".to_string(),
            None => "listening".to_string(),
        };
        if self.live {
            words = format!("live · {words}");
        }
        frame.render_widget(
            Paragraph::new(Span::styled(format!("# {words}"), roles.comment())),
            inset(head),
        );
        self.draw_table(frame, table, roles);
        let left = self.state_line(roles);
        status_bar::draw(frame, rule, bar, roles, left, &HINTS);
    }

    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let (dot, words, tone) = match &self.heard {
            _ if self.scanning => {
                let lit = (self.opened.elapsed().as_millis() / PULSE.as_millis()).is_multiple_of(2);
                let dot = if lit { roles.active() } else { roles.comment() };
                (dot, "scanning".to_string(), roles.muted())
            }
            Some((Ok(networks), took)) => (
                roles.good(),
                format!("{} heard · {:.1}s", networks.len(), took.as_secs_f32()),
                roles.muted(),
            ),
            Some((Err(why), _)) => (roles.warning(), why.clone(), roles.warning()),
            None => (roles.comment(), "scanning".to_string(), roles.muted()),
        };
        Line::from(vec![Span::styled("● ", dot), Span::styled(words, tone)])
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let networks = self.rows();
        let index = self.index(&networks);
        // Neighbours count only what the last scan heard, not what has gone out of range.
        let heard = self.heard();
        let crowds: HashMap<String, Crowd> = heard
            .iter()
            .map(trend::key)
            .zip(channels::crowds(heard))
            .collect();
        let rows: Vec<Row<'static>> = networks
            .iter()
            .enumerate()
            .map(|(i, (network, reading))| {
                let crowd = crowds.get(&trend::key(network)).copied();
                row(network, crowd, *reading, self.live, i == index, roles)
            })
            .collect();
        let empty = rows.is_empty();

        let mut titles = vec!["ssid", "ch", "band", "width", "signal"];
        let mut widths = vec![
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(7),
        ];
        if self.live {
            titles.push("trend");
            widths.push(Constraint::Length(15));
        }
        titles.extend(["neighbours", "air", "bssid"]);
        widths.extend([
            Constraint::Length(18),
            Constraint::Length(30),
            Constraint::Length(17),
        ]);
        let header = Row::new(
            titles
                .into_iter()
                .map(|title| Cell::from(Span::styled(title, roles.comment()))),
        );
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(2)
            .highlight_symbol(" ".repeat(INSET as usize + 2))
            .highlight_spacing(HighlightSpacing::Always)
            .row_highlight_style(roles.band());
        self.table.select((!empty).then_some(index));
        frame.render_stateful_widget(table, area, &mut self.table);

        if area.height > 0 {
            frame.buffer_mut()[(area.x + INSET, area.y)]
                .set_char('#')
                .set_style(roles.comment());
        }
        if empty && area.height > 1 && matches!(self.heard, Some((Ok(_), _))) {
            frame.render_widget(
                Paragraph::new(Span::styled("# nothing heard", roles.comment())),
                inset(Rect {
                    y: area.y + 1,
                    height: 1,
                    ..area
                }),
            );
        }
    }
}

/// One network's row. Live, a trend cell follows the signal: the dBm gained or lost since the
/// scan before, and the peak so far.
fn row(
    network: &Network,
    crowd: Option<Crowd>,
    reading: Option<Reading>,
    live: bool,
    active: bool,
    roles: &Roles,
) -> Row<'static> {
    let gone = reading.is_some_and(|reading| !reading.heard);
    let mut ssid = vec![match &network.ssid {
        Some(name) if gone => Span::styled(name.clone(), roles.comment()),
        Some(name) if active => Span::styled(name.clone(), roles.active()),
        Some(name) => Span::styled(name.clone(), roles.text()),
        None => Span::styled("hidden", roles.comment()),
    }];
    if network.connected {
        ssid.push(Span::styled(" · joined", roles.good()));
    }
    let neighbours = match crowd {
        Some(crowd) if crowd.overlapping > 0 => Span::styled(crowd.words(), roles.warning()),
        Some(crowd) if crowd.same > 0 => Span::styled(crowd.words(), roles.muted()),
        Some(crowd) => Span::styled(crowd.words(), roles.good()),
        None => Span::styled("—", roles.comment()),
    };
    let width = width(network);
    let width_style = if network.width_mhz.is_some() {
        roles.muted()
    } else {
        roles.comment()
    };
    let bssid = network.bssid.clone().unwrap_or_else(|| "—".to_string());
    let mut cells = vec![
        Cell::from(Line::from(ssid)),
        Cell::from(Span::styled(
            channels::channel(network.freq_mhz).to_string(),
            roles.address(),
        )),
        Cell::from(Span::styled(
            Band::of(network.freq_mhz).name(),
            roles.muted(),
        )),
        Cell::from(Span::styled(width, width_style)),
        Cell::from(Span::styled(
            format!("{} dBm", network.signal_dbm),
            if gone { roles.comment() } else { roles.muted() },
        )),
    ];
    if live {
        let trend = match reading {
            Some(Reading {
                heard: false, peak, ..
            }) => vec![Span::styled(format!("gone · peak {peak}"), roles.comment())],
            Some(Reading {
                change: Some(change),
                peak,
                ..
            }) => {
                let (words, tone) = match change {
                    1.. => (format!("+{change}"), roles.good()),
                    0 => ("±0".to_string(), roles.comment()),
                    _ => (change.to_string(), roles.warning()),
                };
                vec![
                    Span::styled(words, tone),
                    Span::styled(format!(" · peak {peak}"), roles.comment()),
                ]
            }
            _ => vec![Span::styled("new", roles.comment())],
        };
        cells.push(Cell::from(Line::from(trend)));
    }
    cells.extend([
        Cell::from(neighbours),
        Cell::from(Span::styled(
            channels::strip(network.freq_mhz, network.width_mhz),
            roles.muted(),
        )),
        Cell::from(Span::styled(bssid, roles.comment())),
    ]);
    Row::new(cells)
}
