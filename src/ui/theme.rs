//! Colours as named roles from the cavecomputing design system's tokens, in the terminal's own
//! colours. Colours anywhere else on screen come from `Roles`, never hex; the printed report
//! keeps its own in `report/sheet.rs`.

use clap::builder::styling::{AnsiColor, Style as HelpStyle, Styles};
use ratatui::style::{Color, Modifier, Style};

pub struct Roles {
    /// The status bar.
    pub bg_page: Color,
    /// The band under the active row.
    pub bg_inset: Color,
    /// The line above the status bar.
    pub border: Color,
    pub fg: Color,
    pub fg_muted: Color,
    /// `#` comments and placeholders only: below 4.5:1, so never for something to act on.
    pub fg_subtle: Color,
    /// Yellow: the prompt, the active row, a match.
    pub accent_text: Color,
    /// Blue: addresses.
    pub accent_cool: Color,
    /// Warm (magenta on screen): anything that went wrong, always with words.
    /// There is no red.
    pub accent_warm: Color,
    /// Aqua: a host that answered, a copy that worked.
    pub accent_aqua: Color,
}

/// The screen, in the terminal's own colours: its background and text untouched, so the screen
/// sits in the terminal instead of as a block on it, and its 16-colour palette (which the
/// terminal's theme sets) for the accents. The subtle text and the border are the plain colour
/// made faint (see `comment` and `rule`), not bright black: Solarized Dark, a common theme on
/// macOS, makes bright black its background, which hid every comment, heading and hint.
pub const TERMINAL: Roles = Roles {
    bg_page: Color::Reset,
    bg_inset: Color::DarkGray,
    border: Color::Reset,
    fg: Color::Reset,
    fg_muted: Color::Reset,
    fg_subtle: Color::Reset,
    accent_text: Color::Yellow,
    accent_cool: Color::Blue,
    // The palette has no orange, and red is out.
    accent_warm: Color::Magenta,
    accent_aqua: Color::Cyan,
};

/// The help, in the same terminal colours as the screen: headings yellow like the prompt, flags
/// blue, values aqua, and mistakes warm (there is no red). clap leaves the colour out when the
/// help goes to a pipe or a file, or when `NO_COLOR` is set.
pub const HELP: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Yellow.on_default().bold())
    .literal(AnsiColor::Blue.on_default().bold())
    .placeholder(AnsiColor::Cyan.on_default())
    .valid(AnsiColor::Cyan.on_default())
    .invalid(AnsiColor::Magenta.on_default())
    .error(AnsiColor::Magenta.on_default().bold())
    .context(HelpStyle::new().dimmed());

impl Roles {
    pub fn text(&self) -> Style {
        Style::new().fg(self.fg)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.fg_muted)
    }

    /// A `#` line, a placeholder, a dash for nothing. Faint, so it is dimmer than the text in
    /// any theme (a terminal without faint shows it plain).
    pub fn comment(&self) -> Style {
        Style::new().fg(self.fg_subtle).add_modifier(Modifier::DIM)
    }

    /// The line above the status bar and the axes of a graph.
    pub fn rule(&self) -> Style {
        Style::new().fg(self.border).add_modifier(Modifier::DIM)
    }

    /// The band under the active row. Its text all takes the plain colour, not faint: the band
    /// is bright black and faint text could vanish into it. Bold too, so the row still stands
    /// out in a theme whose bright black is its background.
    pub fn band(&self) -> Style {
        Style::new()
            .bg(self.bg_inset)
            .fg(self.fg)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM)
    }

    pub fn active(&self) -> Style {
        Style::new().fg(self.accent_text)
    }

    /// The part of a cell the query matched.
    pub fn lit(&self) -> Style {
        Style::new()
            .fg(self.accent_text)
            .add_modifier(Modifier::BOLD)
    }

    pub fn address(&self) -> Style {
        Style::new().fg(self.accent_cool)
    }

    pub fn good(&self) -> Style {
        Style::new().fg(self.accent_aqua)
    }

    pub fn warning(&self) -> Style {
        Style::new().fg(self.accent_warm)
    }

    /// Host `i`'s colour on the ping screen, its dot and its graph: aqua, blue, yellow, green,
    /// then the plain colour. Never warm, which means something went wrong.
    pub fn host(&self, i: usize) -> Style {
        let colours = [
            self.accent_aqua,
            self.accent_cool,
            self.accent_text,
            Color::Green,
            self.fg,
        ];
        Style::new().fg(colours[i % colours.len()])
    }
}
