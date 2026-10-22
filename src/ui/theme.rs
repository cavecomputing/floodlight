//! The cavecomputing Gruvbox palette as named roles (from its design system's tokens), mapped to
//! ratatui styles. Colours anywhere else come from `Roles`, never hex.

use crate::os::settings::Theme;
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
    /// Orange: anything that went wrong, always with words. There is no red.
    pub accent_warm: Color,
    /// Aqua: a host that answered, a copy that worked.
    pub accent_aqua: Color,
}

pub const DARK: Roles = Roles {
    bg_page: Color::Rgb(0x1d, 0x20, 0x21),
    bg_surface: Color::Rgb(0x28, 0x28, 0x28),
    bg_inset: Color::Rgb(0x3c, 0x38, 0x36),
    border: Color::Rgb(0x3c, 0x38, 0x36),
    fg: Color::Rgb(0xeb, 0xdb, 0xb2),
    fg_muted: Color::Rgb(0xa8, 0x99, 0x84),
    fg_subtle: Color::Rgb(0x7c, 0x6f, 0x64),
    accent_text: Color::Rgb(0xfa, 0xbd, 0x2f),
    accent_cool: Color::Rgb(0x83, 0xa5, 0x98),
    accent_warm: Color::Rgb(0xfe, 0x80, 0x19),
    accent_aqua: Color::Rgb(0x8e, 0xc0, 0x7c),
};

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
    pub fn of(theme: Theme) -> &'static Roles {
        match theme {
            Theme::Dark => &DARK,
            Theme::Light => &LIGHT,
        }
    }

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
