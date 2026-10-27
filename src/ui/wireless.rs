//! The wireless screen: every network heard, its channel and width, who shares or overlaps it,
//! and a strip of its band so the rows stacked together draw the overlap. Same look as the sweep's
//! screen, without the prompt.

use crate::os::settings::{self, Settings};
use crate::ui::status_bar::{self, Notice};
use crate::ui::theme::Roles;
use crate::ui::{INSET, inset};
use crate::wifi::channels::{self, Band, Crowd};
use crate::wifi::survey::{self, Network};
use ratatui::crossterm::event::{
    self as input, Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, HighlightSpacing, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

const TICK: Duration = Duration::from_millis(50);
const NOTICE_FOR: Duration = Duration::from_millis(2500);
const PULSE: Duration = Duration::from_millis(600);
const HINTS: [&str; 3] = ["^r scan again", "^t theme", "^c quit"];

pub fn run(runtime: &Runtime, settings: Settings) -> anyhow::Result<()> {
    let mut screen = Screen {
        runtime,
        survey: start(runtime),
        heard: None,
        selected: 0,
        table: TableState::default(),
        settings,
        notice: None,
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
    survey: oneshot::Receiver<(Result<Vec<Network>, String>, Duration)>,
    /// None while the survey is out.
    heard: Option<(Result<Vec<Network>, String>, Duration)>,
    selected: usize,
    table: TableState,
    settings: Settings,
    notice: Option<Notice>,
    opened: Instant,
    quit: bool,
}

impl Screen<'_> {
    fn show(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.quit {
            if let Ok(heard) = self.survey.try_recv() {
                self.heard = Some(heard);
            }
            if self
                .notice
                .as_ref()
                .is_some_and(|notice| notice.until <= Instant::now())
            {
                self.notice = None;
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
            (KeyCode::Char('t'), true) => self.switch_theme(),
            (KeyCode::Up, _) | (KeyCode::Char('p'), true) => self.step(-1),
            (KeyCode::Down, _) | (KeyCode::Char('n'), true) => self.step(1),
            (KeyCode::PageUp, _) => self.step(-10),
            (KeyCode::PageDown, _) => self.step(10),
            (KeyCode::Home, _) => self.step(i32::MIN / 2),
            (KeyCode::End, _) => self.step(i32::MAX / 2),
            _ => {}
        }
    }

    fn networks(&self) -> &[Network] {
        match &self.heard {
            Some((Ok(networks), _)) => networks,
            _ => &[],
        }
    }

    fn step(&mut self, by: i32) {
        let last = self.networks().len().saturating_sub(1) as i32;
        self.selected = (self.selected as i32).saturating_add(by).clamp(0, last) as usize;
    }

    fn scan_again(&mut self) {
        if self.heard.is_none() {
            return;
        }
        self.survey = start(self.runtime);
        self.heard = None;
        self.selected = 0;
        self.opened = Instant::now();
    }

    /// Applies and saves at once, like every setting.
    fn switch_theme(&mut self) {
        self.settings.theme = self.settings.theme.other();
        if let Err(why) = settings::save(&self.settings) {
            self.notice = Some(Notice {
                text: format!("theme not saved: {why}"),
                ok: false,
                until: Instant::now() + NOTICE_FOR,
            });
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let roles = Roles::of(self.settings.theme);
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

        let words = match &self.heard {
            Some((Ok(networks), _)) => headline(networks),
            Some((Err(_), _)) => "nothing to show".to_string(),
            None => "listening".to_string(),
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("# {words}"), roles.comment())),
            inset(head),
        );
        self.draw_table(frame, table, roles);
        let left = self.state_line(roles);
        status_bar::draw(frame, rule, bar, roles, left, &HINTS);
    }

    fn state_line(&self, roles: &Roles) -> Line<'static> {
        let (dot, words, tone) = match (&self.notice, &self.heard) {
            (Some(notice), _) => {
                let tone = if notice.ok {
                    roles.good()
                } else {
                    roles.warning()
                };
                (tone, notice.text.clone(), tone)
            }
            (None, Some((Ok(networks), took))) => (
                roles.good(),
                format!("{} heard · {:.1}s", networks.len(), took.as_secs_f32()),
                roles.muted(),
            ),
            (None, Some((Err(why), _))) => (roles.warning(), why.clone(), roles.warning()),
            (None, None) => {
                let lit = (self.opened.elapsed().as_millis() / PULSE.as_millis()).is_multiple_of(2);
                let dot = if lit { roles.active() } else { roles.comment() };
                (dot, "scanning".to_string(), roles.muted())
            }
        };
        Line::from(vec![Span::styled("● ", dot), Span::styled(words, tone)])
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let networks = self.networks();
        let crowds = channels::crowds(networks);
        let rows: Vec<Row<'static>> = networks
            .iter()
            .zip(&crowds)
            .enumerate()
            .map(|(i, (network, crowd))| row(network, *crowd, i == self.selected, roles))
            .collect();
        let empty = rows.is_empty();

        let header = Row::new(
            [
                "ssid",
                "ch",
                "band",
                "width",
                "signal",
                "neighbours",
                "air",
                "bssid",
            ]
            .map(|title| Cell::from(Span::styled(title, roles.comment()))),
        );
        let table = Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(7),
                Constraint::Length(7),
                Constraint::Length(18),
                Constraint::Length(30),
                Constraint::Length(17),
            ],
        )
        .header(header)
        .column_spacing(2)
        .highlight_symbol(" ".repeat(INSET as usize + 2))
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(Style::new().bg(roles.bg_inset));
        self.table.select((!empty).then_some(self.selected));
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

fn row(network: &Network, crowd: Crowd, active: bool, roles: &Roles) -> Row<'static> {
    let mut ssid = vec![match &network.ssid {
        Some(name) if active => Span::styled(name.clone(), roles.active()),
        Some(name) => Span::styled(name.clone(), roles.text()),
        None => Span::styled("hidden", roles.comment()),
    }];
    if network.connected {
        ssid.push(Span::styled(" · joined", roles.good()));
    }
    let neighbours = if crowd.overlapping > 0 {
        roles.warning()
    } else if crowd.same > 0 {
        roles.muted()
    } else {
        roles.good()
    };
    let width = width(network);
    let width_style = if network.width_mhz.is_some() {
        roles.muted()
    } else {
        roles.comment()
    };
    let bssid = network.bssid.clone().unwrap_or_else(|| "—".to_string());
    Row::new(vec![
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
            roles.muted(),
        )),
        Cell::from(Span::styled(crowd.words(), neighbours)),
        Cell::from(Span::styled(
            channels::strip(network.freq_mhz, network.width_mhz),
            roles.muted(),
        )),
        Cell::from(Span::styled(bssid, roles.comment())),
    ])
}
