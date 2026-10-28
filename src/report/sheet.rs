//! The report's page, laid out like a printed documentation page: white Letter paper, a running
//! header and a page count on every page, a title over a strip of facts, headings over a thin rule,
//! and tables with a grey header row and striped rows. No name or logo.

use super::pdf::{self, Font, PAGE_HEIGHT, PAGE_WIDTH, Pdf, Rgb};

/// The paper's colours: near-black text, greys for structure, and one slate blue.
pub const FG: Rgb = (0x1f, 0x23, 0x28);
pub const MUTED: Rgb = (0x59, 0x63, 0x6e);
/// Dashes and words standing in for a missing value.
pub const SUBTLE: Rgb = (0x7d, 0x87, 0x91);
pub const BORDER: Rgb = (0xd1, 0xd9, 0xe0);
/// The facts strip.
const SURFACE: Rgb = (0xf6, 0xf8, 0xfa);
/// A table's header row.
const RAISED: Rgb = (0xf0, 0xf3, 0xf6);
/// Every other row of a table.
const STRIPE: Rgb = (0xf8, 0xf9, 0xfb);
/// The one accent: the charts' lines.
pub const ACCENT: Rgb = (0x2b, 0x5b, 0x84);
/// Addresses.
pub const COOL: Rgb = ACCENT;
/// A host that answered, the network joined, the download.
pub const AQUA: Rgb = (0x1a, 0x7f, 0x37);

/// Today's date and the time now, in this machine's time zone: `October 27, 2026` and `14:05`.
pub fn now() -> (String, String) {
    let now = chrono::Local::now();
    (
        now.format("%B %-d, %Y").to_string(),
        now.format("%H:%M").to_string(),
    )
}

/// The page margins, and the width between them.
pub const LEFT: f32 = 54.0;
pub const WIDTH: f32 = PAGE_WIDTH - 2.0 * LEFT;
const TOP: f32 = 72.0;
const BOTTOM: f32 = PAGE_HEIGHT - 56.0;
/// The running header's baseline.
const HEADER: f32 = 40.0;

const ROW: f32 = 15.0;
const CELL_PAD: f32 = 7.0;

/// A table column: its heading, its share of the width, and whether it holds addresses or MACs
/// (set in the monospaced font) or numbers (set to the right).
pub struct Column {
    pub label: &'static str,
    pub width: f32,
    pub mono: bool,
    pub right: bool,
}

impl Column {
    /// Words, set to the left.
    pub const fn text(label: &'static str, width: f32) -> Column {
        Column {
            label,
            width,
            mono: false,
            right: false,
        }
    }

    /// Addresses, MACs and times, in the monospaced font.
    pub const fn mono(label: &'static str, width: f32) -> Column {
        Column {
            mono: true,
            ..Column::text(label, width)
        }
    }

    /// Numbers, set to the right so their digits line up.
    pub const fn number(label: &'static str, width: f32) -> Column {
        Column {
            right: true,
            ..Column::text(label, width)
        }
    }
}

/// One cell's text and colour.
pub type Cell = (String, Rgb);

/// `text` with a capital first letter: the reports write their titles, facts and subtitles in
/// lowercase, as the screen does.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A report being laid out, top to bottom, a new page whenever the next piece won't fit.
pub struct Sheet {
    pub pdf: Pdf,
    /// How far down the current page the next piece starts.
    pub y: f32,
    /// The running header: what kind of report on the left, its title on the right.
    kind: String,
    title: String,
    /// The left of the footer: when the report was made.
    footer: String,
}

impl Sheet {
    pub fn new(date: &str, time: &str) -> Sheet {
        let mut sheet = Sheet {
            pdf: Pdf::new(),
            y: 0.0,
            kind: String::new(),
            title: String::new(),
            footer: format!("Generated {date}, {time}"),
        };
        sheet.new_page();
        sheet
    }

    fn new_page(&mut self) {
        self.pdf.add_page();
        self.y = TOP;
    }

    /// Starts a new page unless `needed` points still fit on this one.
    pub fn room(&mut self, needed: f32) {
        if self.y + needed > BOTTOM {
            self.new_page();
        }
    }

    /// The title and a line under it, then the facts in a grey strip of small labelled columns.
    /// The kind and the title also head every page.
    pub fn masthead(&mut self, kind: &str, title: &str, subtitle: &str, facts: &[(&str, String)]) {
        let title = capitalized(title);
        self.kind = kind.to_string();
        self.title = title.clone();
        let y = self.y;
        let pdf = &mut self.pdf;
        let title = pdf::fit(Font::SansBold, 20.0, &title, WIDTH);
        pdf.text(LEFT, y + 16.0, Font::SansBold, 20.0, FG, &title);
        pdf.text(
            LEFT,
            y + 32.0,
            Font::Sans,
            10.0,
            MUTED,
            &capitalized(subtitle),
        );
        let (strip, height) = (y + 44.0, 34.0);
        pdf.clipped((LEFT, strip, WIDTH, height, 3.0), BORDER, |pdf| {
            pdf.rect(LEFT, strip, WIDTH, height, SURFACE)
        });
        let mut column = LEFT + 11.0;
        for (label, fact) in facts {
            let label = capitalized(label);
            pdf.text(column, strip + 13.0, Font::Sans, 7.5, MUTED, &label);
            pdf.text(column, strip + 25.0, Font::SansBold, 8.5, FG, fact);
            let label_width = pdf::width(Font::Sans, 7.5, &label);
            column += pdf::width(Font::SansBold, 8.5, fact).max(label_width) + 26.0;
        }
        self.y = strip + height + 14.0;
    }

    /// A line of plain text in place of a table with nothing in it.
    pub fn note(&mut self, text: &str) {
        let lines = pdf::wrap(Font::Sans, 10.0, text, WIDTH);
        self.room(14.0 * lines.len() as f32);
        for line in lines {
            self.pdf
                .text(LEFT, self.y + 10.0, Font::Sans, 10.0, FG, &line);
            self.y += 14.0;
        }
        self.y += 4.0;
    }

    /// A section heading over a thin rule, kept on the page with at least `then` points of what
    /// follows it.
    pub fn heading(&mut self, text: &str, then: f32) {
        self.room(40.0 + then);
        self.y += 16.0;
        let pdf = &mut self.pdf;
        pdf.text(LEFT, self.y + 10.0, Font::SansBold, 12.5, FG, text);
        pdf.line(
            (LEFT, self.y + 16.0),
            (LEFT + WIDTH, self.y + 16.0),
            0.5,
            BORDER,
        );
        self.y += 24.0;
    }

    /// A table in a thin outline, one line a row, cut to fit its column. A table that runs off
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
        self.pdf.clipped((x, y, WIDTH, height, 0.0), BORDER, |pdf| {
            pdf.rect(x, y, WIDTH, ROW, RAISED);
            for i in (1..rows.len()).step_by(2) {
                pdf.rect(x, y + ROW * (i + 1) as f32, WIDTH, ROW, STRIPE);
            }
            pdf.line((x, y + ROW), (x + WIDTH, y + ROW), 0.5, BORDER);
        });
        let header: Vec<Cell> = columns
            .iter()
            .map(|column| (column.label.to_string(), FG))
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
                (true, _) => (Font::SansBold, 8.0),
                (false, true) => (Font::Mono, 8.0),
                (false, false) => (Font::Sans, 8.5),
            };
            let room = column.width * WIDTH - 2.0 * CELL_PAD;
            let text = pdf::fit(font, size, text, room);
            let left = match column.right {
                true => x + column.width * WIDTH - CELL_PAD - pdf::width(font, size, &text),
                false => x + CELL_PAD,
            };
            self.pdf.text(left, top + 10.5, font, size, *ink, &text);
            x += column.width * WIDTH;
        }
    }

    /// The finished file, with the running header and the footer on every page now the count is
    /// known.
    pub fn finish(mut self) -> Vec<u8> {
        let pages = self.pdf.page_count();
        let footer = PAGE_HEIGHT - 30.0;
        let right = |text: &str| LEFT + WIDTH - pdf::width(Font::Sans, 7.5, text);
        let title = pdf::fit(Font::Sans, 7.5, &self.title, WIDTH / 2.0);
        for page in 0..pages {
            let count = format!("Page {} of {pages}", page + 1);
            let pdf = &mut self.pdf;
            pdf.text_on(page, LEFT, HEADER, Font::Sans, 7.5, MUTED, &self.kind);
            pdf.text_on(page, right(&title), HEADER, Font::Sans, 7.5, MUTED, &title);
            pdf.line_on(
                page,
                (LEFT, HEADER + 6.0),
                (LEFT + WIDTH, HEADER + 6.0),
                0.5,
                BORDER,
            );
            pdf.text_on(page, LEFT, footer, Font::Sans, 7.5, MUTED, &self.footer);
            pdf.text_on(page, right(&count), footer, Font::Sans, 7.5, MUTED, &count);
        }
        self.pdf.finish()
    }
}
