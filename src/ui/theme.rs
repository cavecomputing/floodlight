//! Colours as named roles from the cavecomputing design system's tokens: the screen's, in the
//! terminal's own colours, and the Gruvbox light palette the printed report uses. Colours
//! anywhere else come from `Roles`, never hex.

use ratatui::style::{Color, Modifier, Style};

pub struct Roles {
    /// The status bar.
    pub bg_page: Color,
    /// The screen.
    pub bg_surface: Color,
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
    /// Warm (orange in print, magenta on screen): anything that went wrong, always with words.
    /// There is no red.
    pub accent_warm: Color,
    /// Aqua: a host that answered, a copy that worked.
    pub accent_aqua: Color,
}

/// The screen, in the terminal's own colours: its background and text untouched, so the screen
/// sits in the terminal instead of as a block on it, and its 16-colour palette (which the
/// terminal's theme sets) for the accents. Bright black is the terminal's dim colour.
pub const TERMINAL: Roles = Roles {
    bg_page: Color::Reset,
    bg_surface: Color::Reset,
    bg_inset: Color::DarkGray,
    border: Color::DarkGray,
    fg: Color::Reset,
    fg_muted: Color::Reset,
    fg_subtle: Color::DarkGray,
    accent_text: Color::Yellow,
    accent_cool: Color::Blue,
    // The palette has no orange, and red is out.
    accent_warm: Color::Magenta,
    accent_aqua: Color::Cyan,
};

/// The Gruvbox light theme, for the printed report.
pub const LIGHT: Roles = Roles {
    bg_page: Color::Rgb(0xf9, 0xf5, 0xd7),
    bg_surface: Color::Rgb(0xfb, 0xf1, 0xc7),
    bg_inset: Color::Rgb(0xeb, 0xdb, 0xb2),
    border: Color::Rgb(0xe0, 0xd3, 0xac),
    fg: Color::Rgb(0x3c, 0x38, 0x36),
    fg_muted: Color::Rgb(0x66, 0x5c, 0x54),
    fg_subtle: Color::Rgb(0x7c, 0x6f, 0x64),
    accent_text: Color::Rgb(0x8a, 0x5a, 0x0e),
    accent_cool: Color::Rgb(0x07, 0x66, 0x78),
    accent_warm: Color::Rgb(0xaf, 0x3a, 0x03),
    accent_aqua: Color::Rgb(0x37, 0x6b, 0x4b),
};

impl Roles {
    pub fn text(&self) -> Style {
        Style::new().fg(self.fg)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.fg_muted)
    }

    /// A `#` line, a placeholder, a dash for nothing.
    pub fn comment(&self) -> Style {
        Style::new().fg(self.fg_subtle)
    }

    /// The band under the active row. Its text all takes the plain colour: the dim one that
    /// comments use is the band's own colour and would vanish into it.
    pub fn band(&self) -> Style {
        Style::new().bg(self.bg_inset).fg(self.fg)
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
}
