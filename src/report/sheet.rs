//! The report's page, laid out like the design system's print documents: a cream Letter page, a
//! header card with a yellow top edge, a summary with a yellow left edge, small capital section
//! headings, tables in rounded outlines, and a footer line with the page count. No name or logo.

use super::pdf::{self, Font, PAGE_HEIGHT, PAGE_WIDTH, Pdf, Rgb};
use crate::ui::theme::LIGHT;
use ratatui::style::Color;

/// The light theme's roles, as the print documents use them.
pub const PAGE: Rgb = rgb(LIGHT.bg_page);
pub const SURFACE: Rgb = rgb(LIGHT.bg_surface);
pub const BORDER: Rgb = rgb(LIGHT.border);
pub const FG: Rgb = rgb(LIGHT.fg);
pub const MUTED: Rgb = rgb(LIGHT.fg_muted);
pub const SUBTLE: Rgb = rgb(LIGHT.fg_subtle);
pub const COOL: Rgb = rgb(LIGHT.accent_cool);
pub const AQUA: Rgb = rgb(LIGHT.accent_aqua);
/// The yellow of the print documents' edges and headings: paper takes a brighter one than the
/// screen's `accent-text`.
pub const ACCENT: Rgb = (0xb5, 0x76, 0x14);
/// A table's header row.
pub const RAISED: Rgb = (0xf2, 0xe5, 0xbc);
/// A table's rows: a shade lighter than the surface, so the table reads as paper on the page.
pub const CELL: Rgb = (0xff, 0xfb, 0xe5);

const fn rgb(colour: Color) -> Rgb {
    match colour {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

/// Today's date and the time now, in this machine's time zone: `October 27, 2026` and `14:05`.
pub fn now() -> (String, String) {
    let now = chrono::Local::now();
    (
        now.format("%B %-d, %Y").to_string(),
        now.format("%H:%M").to_string(),
    )
}

/// The page margins, and the width between them.
pub const LEFT: f32 = 43.0;
pub const WIDTH: f32 = PAGE_WIDTH - 2.0 * LEFT;
const TOP: f32 = 40.0;
const BOTTOM: f32 = PAGE_HEIGHT - 50.0;

const ROW: f32 = 18.0;
const CELL_PAD: f32 = 8.0;
const RADIUS: f32 = 8.0;

/// A table column: its heading, its share of the width, and whether it holds addresses or MACs
/// (set in the monospaced font) or numbers (set to the right).
pub struct Column {
    pub label: &'static str,
    pub width: f32,
    pub mono: bool,
    pub right: bool,
}

/// One cell's text and colour.
pub type Cell = (String, Rgb);

/// A report being laid out, top to bottom, a new page whenever the next piece won't fit.
pub struct Sheet {
    pub pdf: Pdf,
    /// How far down the current page the next piece starts.
    pub y: f32,
    /// The left of the footer: what the report is and when it was made.
    footer: String,
}

impl Sheet {
    pub fn new(footer: String) -> Sheet {
        let mut sheet = Sheet {
            pdf: Pdf::new(),
            y: 0.0,
            footer,
        };
        sheet.new_page();
        sheet
    }

    fn new_page(&mut self) {
        self.pdf.add_page();
        self.pdf.rect(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT, PAGE);
        self.y = TOP;
    }

    /// Starts a new page unless `needed` points still fit on this one.
    pub fn room(&mut self, needed: f32) {
        if self.y + needed > BOTTOM {
            self.new_page();
        }
    }

    /// The header card: what kind of report, its title and a line under it, then the facts
    /// in a row of small labelled columns.
    pub fn masthead(&mut self, kind: &str, title: &str, subtitle: &str, facts: &[(&str, String)]) {
        let (x, y, height) = (LEFT, self.y, 104.0);
        self.pdf
            .clipped((x, y, WIDTH, height, 10.0), BORDER, |pdf| {
                pdf.rect(x, y, WIDTH, height, SURFACE);
                pdf.rect(x, y, WIDTH, 4.0, ACCENT);
            });
        let inner = x + 20.0;
        let pdf = &mut self.pdf;
        pdf.text(
            inner,
            y + 22.0,
            Font::MonoBold,
            7.5,
            SUBTLE,
            &kind.to_uppercase(),
        );
        let title = pdf::fit(Font::SansBold, 18.0, title, WIDTH - 40.0);
        pdf.text(inner, y + 46.0, Font::SansBold, 18.0, FG, &title);
        pdf.text(inner, y + 62.0, Font::Sans, 10.5, MUTED, subtitle);
        pdf.line(
            (inner, y + 72.0),
            (x + WIDTH - 20.0, y + 72.0),
            0.75,
            BORDER,
        );
        let mut column = inner;
        for (label, fact) in facts {
            pdf.text(
                column,
                y + 84.0,
                Font::MonoBold,
                6.8,
                SUBTLE,
                &label.to_uppercase(),
            );
            pdf.text(column, y + 95.0, Font::Sans, 8.5, FG, fact);
            let label_width = pdf::width(Font::MonoBold, 6.8, label);
            column += pdf::width(Font::Sans, 8.5, fact).max(label_width) + 22.0;
        }
        self.y += height + 14.0;
    }

    /// The few sentences a reader takes away, in a card with a yellow left edge.
    pub fn summary(&mut self, text: &str) {
        let lines = pdf::wrap(Font::Sans, 10.0, text, WIDTH - 32.0);
        let height = 20.0 + 14.0 * lines.len() as f32;
        self.room(height);
        let (x, y) = (LEFT, self.y);
        self.pdf
            .clipped((x, y, WIDTH, height, RADIUS), BORDER, |pdf| {
                pdf.rect(x, y, WIDTH, height, SURFACE);
                pdf.rect(x, y, 4.0, height, ACCENT);
            });
        for (i, line) in lines.iter().enumerate() {
            let baseline = y + 24.0 + 14.0 * i as f32;
            self.pdf
                .text(x + 16.0, baseline, Font::Sans, 10.0, FG, line);
        }
        self.y += height + 4.0;
    }

    /// A section heading, kept on the page with at least `then` points of what follows it.
    pub fn heading(&mut self, text: &str, then: f32) {
        self.room(34.0 + then);
        self.y += 16.0;
        let pdf = &mut self.pdf;
        pdf.text(
            LEFT,
            self.y + 8.0,
            Font::MonoBold,
            8.0,
            ACCENT,
            &text.to_uppercase(),
        );
        pdf.line(
            (LEFT, self.y + 13.0),
            (LEFT + WIDTH, self.y + 13.0),
            0.75,
            BORDER,
        );
        self.y += 20.0;
    }

    /// A table in a rounded outline, one line a row, cut to fit its column. A table that runs off
    /// the page carries on in a new outline on the next one, under its header row again.
    pub fn table(&mut self, columns: &[Column], rows: &[Vec<Cell>]) {
        let mut rest = rows;
        while !rest.is_empty() {
            self.room(2.0 * ROW + 2.0);
            let fits = ((BOTTOM - self.y - ROW) / ROW).floor() as usize;
            let (these, after) = rest.split_at(fits.min(rest.len()));
            self.table_part(columns, these);
            rest = after;
        }
    }

    fn table_part(&mut self, columns: &[Column], rows: &[Vec<Cell>]) {
        let (x, y) = (LEFT, self.y);
        let height = ROW * (rows.len() + 1) as f32;
        self.pdf
            .clipped((x, y, WIDTH, height, RADIUS), BORDER, |pdf| {
                pdf.rect(x, y, WIDTH, height, CELL);
                pdf.rect(x, y, WIDTH, ROW, RAISED);
                for i in 0..rows.len() {
                    let line = y + ROW * (i + 1) as f32;
                    pdf.line((x, line), (x + WIDTH, line), 0.75, BORDER);
                }
            });
        let header: Vec<Cell> = columns
            .iter()
            .map(|column| (column.label.to_uppercase(), MUTED))
            .collect();
        self.cells(columns, &header, y, true);
        for (i, row) in rows.iter().enumerate() {
            self.cells(columns, row, y + ROW * (i + 1) as f32, false);
        }
        self.y += height + 8.0;
    }

    fn cells(&mut self, columns: &[Column], row: &[Cell], top: f32, header: bool) {
        let mut x = LEFT;
        for (column, (text, ink)) in columns.iter().zip(row) {
            let (font, size) = match (header, column.mono) {
                (true, _) => (Font::MonoBold, 6.8),
                (false, true) => (Font::Mono, 8.0),
                (false, false) => (Font::Sans, 8.5),
            };
            let room = column.width * WIDTH - 2.0 * CELL_PAD;
            let text = pdf::fit(font, size, text, room);
            let left = match column.right {
                true => x + column.width * WIDTH - CELL_PAD - pdf::width(font, size, &text),
                false => x + CELL_PAD,
            };
            self.pdf.text(left, top + 12.0, font, size, *ink, &text);
            x += column.width * WIDTH;
        }
    }

    /// The finished file, with the footer on every page now the count is known.
    pub fn finish(mut self) -> Vec<u8> {
        let pages = self.pdf.page_count();
        let baseline = PAGE_HEIGHT - 28.0;
        for page in 0..pages {
            let count = format!("page {} of {pages}", page + 1);
            let right = LEFT + WIDTH - pdf::width(Font::Sans, 7.5, &count);
            let pdf = &mut self.pdf;
            pdf.text_on(page, LEFT, baseline, Font::Sans, 7.5, SUBTLE, &self.footer);
            pdf.text_on(page, right, baseline, Font::Sans, 7.5, SUBTLE, &count);
        }
        self.pdf.finish()
    }
}
