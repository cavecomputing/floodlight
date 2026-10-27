//! The search screen: the sweep's rows under a `>` prompt, filtered as you type, and the status
//! bar. Laid out the way the design's desktop section draws a panel: `#` lines are comments, `>`
//! is the prompt with no box around it, the active row is a full-width band, and nothing says what
//! the screen is.

use crate::os::clipboard;
use crate::scan::hosts::{Column, Hit, Host, Hosts, Query, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep::{self, Event};
use crate::ui::status_bar::{self, Notice};
use crate::ui::theme::{Roles, TERMINAL};
use crate::ui::{INSET, inset};
use ratatui::crossterm::event::{
    self as input, Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, HighlightSpacing, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedReceiver;

/// How often the screen looks for new rows and key presses.
const TICK: Duration = Duration::from_millis(50);
const NOTICE_FOR: Duration = Duration::from_millis(2500);
/// The sweeping dot's pulse, the screen's only looping motion.
const PULSE: Duration = Duration::from_millis(600);
const PLACEHOLDER: &str = "type part of an ip, mac, maker or name";
const HINTS: [&str; 5] = [
    "enter copy ip",
    "tab copy mac",
    "^r sweep again",
    "esc clear",
    "^c quit",
];

pub fn run(runtime: &Runtime, subnet: Subnet) -> anyhow::Result<()> {
    let mut app = App::new(runtime, subnet);
    let mut terminal = ratatui::init();
    let outcome = app.show(&mut terminal);
    ratatui::restore();
    outcome
}

struct App<'a> {
    runtime: &'a Runtime,
    subnet: Subnet,
    events: UnboundedReceiver<Event>,
    hosts: Hosts,
    filter: String,
    /// The row the band is on, by address, so it stays put as rows arrive above it.
    selected: Option<Ipv4Addr>,
    table: TableState,
    notice: Option<Notice>,
    opened: Instant,
    quit: bool,
}

impl<'a> App<'a> {
    fn new(runtime: &'a Runtime, subnet: Subnet) -> App<'a> {
        let events = sweep::start(runtime, subnet.net);
        let hosts = Hosts::new(subnet.net.hosts().count());
        App {
            runtime,
            subnet,
            events,
            hosts,
            filter: String::new(),
            selected: None,
            table: TableState::default(),
            notice: None,
            opened: Instant::now(),
            quit: false,
        }
    }

    fn show(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.quit {
            while let Ok(event) = self.events.try_recv() {
                self.hosts.apply(event);
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
            (KeyCode::Char('c'), true) | (KeyCode::Char('q'), true) => self.quit = true,
            (KeyCode::Esc, _) if self.filter.is_empty() => self.quit = true,
            (KeyCode::Esc, _) | (KeyCode::Char('u'), true) => self.retype(String::new()),
            (KeyCode::Enter, _) => self.copy(Column::Ip),
            (KeyCode::Tab, _) => self.copy(Column::Mac),
            (KeyCode::Char('r'), true) => self.sweep_again(),
            (KeyCode::Up, _) | (KeyCode::Char('p'), true) => self.step(-1),
            (KeyCode::Down, _) | (KeyCode::Char('n'), true) => self.step(1),
            (KeyCode::PageUp, _) => self.step(-10),
            (KeyCode::PageDown, _) => self.step(10),
            (KeyCode::Home, _) => self.step(i32::MIN / 2),
            (KeyCode::End, _) => self.step(i32::MAX / 2),
            (KeyCode::Backspace, _) => {
                let mut filter = self.filter.clone();
                filter.pop();
                self.retype(filter);
            }
            (KeyCode::Char(c), false) if c.is_ascii_graphic() || c == ' ' => {
                let mut filter = self.filter.clone();
                filter.push(c);
                self.retype(filter);
            }
            _ => {}
        }
    }

    /// A new filter starts the band back at the first row.
    fn retype(&mut self, filter: String) {
        self.filter = filter;
        self.selected = None;
        self.table = TableState::default();
    }

    fn visible(&self) -> Vec<(&Host, Option<Hit>)> {
        let query = Query::new(&self.filter);
        self.hosts
            .rows
            .values()
            .filter_map(|host| {
                if query.is_empty() {
                    Some((host, None))
                } else {
                    host.hit(&query).map(|hit| (host, Some(hit)))
                }
            })
            .collect()
    }

    /// The band's row among the visible ones.
    fn index(&self, visible: &[(&Host, Option<Hit>)]) -> usize {
        visible
            .iter()
            .position(|(host, _)| Some(host.ip) == self.selected)
            .unwrap_or(0)
    }

    fn step(&mut self, by: i32) {
        let ips: Vec<Ipv4Addr> = self.visible().iter().map(|(host, _)| host.ip).collect();
        if ips.is_empty() {
            return;
        }
        let at = ips
            .iter()
            .position(|ip| Some(*ip) == self.selected)
            .unwrap_or(0) as i32;
        let to = (at.saturating_add(by)).clamp(0, ips.len() as i32 - 1) as usize;
        self.selected = Some(ips[to]);
    }

    fn copy(&mut self, column: Column) {
        let visible = self.visible();
        let Some((host, _)) = visible.get(self.index(&visible)) else {
            self.notice("nothing to copy yet", false);
            return;
        };
        let text = match column {
            Column::Mac => match &host.mac {
                Some(mac) => mac.clone(),
                None => {
                    let ip = host.ip;
                    self.notice(&format!("no mac for {ip} yet"), false);
                    return;
                }
            },
            _ => host.ip.to_string(),
        };
        match clipboard::copy(&text) {
            Ok(()) => self.notice(&format!("copied {text}"), true),
            Err(why) => self.notice(&format!("copy failed: {why}"), false),
        }
    }

    fn notice(&mut self, text: &str, ok: bool) {
        self.notice = Some(Notice {
            text: text.to_string(),
            ok,
            until: Instant::now() + NOTICE_FOR,
        });
    }

    fn sweep_again(&mut self) {
        self.events = sweep::start(self.runtime, self.subnet.net);
        self.hosts = Hosts::new(self.subnet.net.hosts().count());
        self.opened = Instant::now();
        self.selected = None;
        self.table = TableState::default();
    }

    fn draw(&mut self, frame: &mut Frame) {
        let roles = &TERMINAL;
        let area = frame.area();
        frame.render_widget(
            Block::new().style(Style::new().bg(roles.bg_surface).fg(roles.fg)),
            area,
        );
        let [_, head, prompt, _, table, rule, bar] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("# ", roles.comment()),
                Span::styled(self.headline(), roles.comment()),
            ])),
            inset(head),
        );

        let typed = if self.filter.is_empty() {
            Span::styled(PLACEHOLDER, roles.comment())
        } else {
            Span::styled(self.filter.clone(), roles.text())
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled("> ", roles.active()), typed])),
            inset(prompt),
        );
        frame.set_cursor_position(Position::new(
            prompt.x + INSET + 2 + self.filter.len() as u16,
            prompt.y,
        ));

        self.draw_table(frame, table, roles);

        let left = self.state_line(roles);
        status_bar::draw(frame, rule, bar, roles, left, &HINTS);
    }

    /// `192.168.1.0/24 · en0 · 254 addresses`
    fn headline(&self) -> String {
        let mut parts = vec![self.subnet.net.to_string()];
        if let Some(interface) = &self.subnet.interface {
            parts.push(interface.clone());
        }
        parts.push(format!("{} addresses", self.hosts.total));
        parts.join(" · ")
    }

    /// The status bar's left side: a notice if one is up, else the sweep's state with its dot.
    fn state_line(&self, roles: &Roles) -> Line<'static> {
        if let Some(notice) = &self.notice {
            let tone = if notice.ok {
                roles.good()
            } else {
                roles.warning()
            };
            return Line::from(vec![
                Span::styled("● ", tone),
                Span::styled(notice.text.clone(), tone),
            ]);
        }
        match self.hosts.took {
            Some(took) => Line::from(vec![
                Span::styled("● ", roles.good()),
                Span::styled(
                    format!("{} up · {:.1}s", self.hosts.rows.len(), took.as_secs_f32()),
                    roles.muted(),
                ),
            ]),
            None => {
                let lit = (self.opened.elapsed().as_millis() / PULSE.as_millis()).is_multiple_of(2);
                let dot = if lit { roles.active() } else { roles.comment() };
                Line::from(vec![
                    Span::styled("● ", dot),
                    Span::styled(
                        format!("sweeping {} of {}", self.hosts.probed, self.hosts.total),
                        roles.muted(),
                    ),
                ])
            }
        }
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect, roles: &Roles) {
        let visible = self.visible();
        let index = self.index(&visible);
        let empty = visible.is_empty();
        let rows: Vec<Row<'static>> = visible
            .iter()
            .enumerate()
            .map(|(i, (host, hit))| {
                row(
                    host,
                    *hit,
                    i == index,
                    self.subnet.own == Some(host.ip),
                    roles,
                )
            })
            .collect();
        drop(visible);

        let header = Row::new(
            ["ip", "mac", "maker", "name", "ping"]
                .map(|title| Cell::from(Span::styled(title, roles.comment()))),
        );
        let table = Table::new(
            rows,
            [
                Constraint::Length(15),
                Constraint::Length(17),
                Constraint::Fill(3),
                Constraint::Fill(3),
                Constraint::Length(11),
            ],
        )
        .header(header)
        .column_spacing(2)
        // The band's gutter is the inset plus the two columns `# ` and `> ` take above.
        .highlight_symbol(" ".repeat(INSET as usize + 2))
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(roles.band());
        self.table.select((!empty).then_some(index));
        frame.render_stateful_widget(table, area, &mut self.table);

        if area.height > 0 && area.width > INSET {
            frame.buffer_mut()[(area.x + INSET, area.y)]
                .set_char('#')
                .set_style(roles.comment());
        }
        if empty && area.height > 1 {
            let why = if !self.filter.is_empty() {
                "nothing matches"
            } else if self.hosts.done() {
                "nothing answered"
            } else {
                "nothing has answered yet"
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("# ", roles.comment()),
                    Span::styled(why, roles.comment()),
                ])),
                inset(Rect {
                    y: area.y + 1,
                    height: 1,
                    ..area
                }),
            );
        }
    }
}

fn row(host: &Host, hit: Option<Hit>, active: bool, own: bool, roles: &Roles) -> Row<'static> {
    let range = |column: Column| match hit {
        Some(hit) if hit.column == column && hit.end > hit.start => Some((hit.start, hit.end)),
        _ => None,
    };
    let ip = cell(
        &host.ip.to_string(),
        if active {
            roles.active()
        } else {
            roles.address()
        },
        range(Column::Ip),
        roles,
    );
    let mac = match &host.mac {
        Some(mac) => cell(mac, roles.text(), range(Column::Mac), roles),
        None => dash(roles),
    };
    let maker = match &host.vendor {
        Vendor::Known(name) => cell(name, roles.muted(), range(Column::Vendor), roles),
        Vendor::Randomized => Cell::from(Span::styled("randomized", roles.comment())),
        Vendor::Unknown => dash(roles),
    };
    let name = match &host.name {
        Some(name) => cell(name, roles.muted(), range(Column::Name), roles),
        None if own => Cell::from(Span::styled("this machine", roles.comment())),
        None => dash(roles),
    };
    let ping = match host.latency_ms {
        Some(ms) => Cell::from(Line::from(vec![
            Span::styled("● ", roles.good()),
            Span::styled(format!("{ms:.1} ms"), roles.muted()),
        ])),
        None => Cell::from(Line::from(vec![
            Span::styled("· ", roles.comment()),
            Span::styled("no reply", roles.comment()),
        ])),
    };
    Row::new(vec![ip, mac, maker, name, ping])
}

/// The text in its style, with the matched part lit.
fn cell(text: &str, base: Style, lit: Option<(usize, usize)>, roles: &Roles) -> Cell<'static> {
    match lit {
        Some((start, end)) if end <= text.len() => Cell::from(Line::from(vec![
            Span::styled(text[..start].to_string(), base),
            Span::styled(text[start..end].to_string(), roles.lit()),
            Span::styled(text[end..].to_string(), base),
        ])),
        _ => Cell::from(Span::styled(text.to_string(), base)),
    }
}

/// An empty cell, as the design's tables mark one.
fn dash(roles: &Roles) -> Cell<'static> {
    Cell::from(Span::styled("—", roles.comment()))
}
