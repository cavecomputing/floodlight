//! A PDF file written by hand: pages of shapes and text in the PDF's own built-in fonts
//! (Helvetica and Courier, which every PDF reader carries), so nothing is embedded and no crate
//! is needed. Coordinates are points from the top-left corner, 72 to the inch.

/// US Letter.
pub const PAGE_WIDTH: f32 = 612.0;
pub const PAGE_HEIGHT: f32 = 792.0;

pub type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Font {
    Sans,
    SansBold,
    Mono,
    MonoBold,
}

impl Font {
    /// The resource name in the content stream, and the built-in font it stands for.
    const ALL: [(Font, &'static str, &'static str); 4] = [
        (Font::Sans, "F1", "Helvetica"),
        (Font::SansBold, "F2", "Helvetica-Bold"),
        (Font::Mono, "F3", "Courier"),
        (Font::MonoBold, "F4", "Courier-Bold"),
    ];

    fn resource(self) -> &'static str {
        Font::ALL
            .iter()
            .find(|(font, ..)| *font == self)
            .map(|(_, resource, _)| *resource)
            .expect("every font is in ALL")
    }
}

/// Character widths in thousandths of the font size for ' ' to '~', from Adobe's metrics.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// The byte for a character in the built-in fonts' encoding (WinAnsi): ASCII and Latin-1 as
/// they are, the few typographic marks the report uses, `?` for anything else.
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' | '\u{a0}'..='\u{ff}' => c as u8,
        '—' => 0x97,
        '–' => 0x96,
        '…' => 0x85,
        '‘' | '’' => b'\'',
        '“' | '”' => b'"',
        _ => b'?',
    }
}

/// How wide the text is set in the font at this size, in points.
pub fn width(font: Font, size: f32, text: &str) -> f32 {
    let thousandths: u32 = text
        .chars()
        .map(|c| {
            let byte = win_ansi(c);
            let table = match font {
                Font::Mono | Font::MonoBold => return 600,
                Font::Sans => &HELVETICA,
                Font::SansBold => &HELVETICA_BOLD,
            };
            match byte {
                b' '..=b'~' => table[(byte - b' ') as usize] as u32,
                0x85 | 0x97 => 1000,
                0xb7 => 278,
                _ => 556,
            }
        })
        .sum();
    thousandths as f32 * size / 1000.0
}

/// The text cut to fit the width, ending in `…` when anything was cut.
pub fn fit(font: Font, size: f32, text: &str, max: f32) -> String {
    if width(font, size, text) <= max {
        return text.to_string();
    }
    let mut cut = text.to_string();
    while !cut.is_empty() && width(font, size, &cut) + width(font, size, "…") > max {
        cut.pop();
    }
    format!("{}…", cut.trim_end())
}

/// The text broken into lines no wider than `max`, between words.
pub fn wrap(font: Font, size: f32, text: &str, max: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if width(font, size, &format!("{line} {word}")) <= max => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The document: one content stream per page, drawn into in order (later on top).
pub struct Pdf {
    pages: Vec<Vec<u8>>,
}

impl Pdf {
    pub fn new() -> Pdf {
        Pdf { pages: Vec::new() }
    }

    pub fn add_page(&mut self) {
        self.pages.push(Vec::new());
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn put(&mut self, page: usize, operators: &str) {
        self.pages[page].extend_from_slice(operators.as_bytes());
        self.pages[page].push(b'\n');
    }

    fn last(&self) -> usize {
        self.pages.len() - 1
    }

    /// A filled rectangle on the current page.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgb) {
        let ops = format!(
            "{} {:.2} {:.2} {:.2} {:.2} re f",
            colour(fill, "rg"),
            x,
            PAGE_HEIGHT - y - h,
            w,
            h
        );
        let page = self.last();
        self.put(page, &ops);
    }

    /// A straight line.
    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), thickness: f32, stroke: Rgb) {
        self.line_on(self.last(), from, to, thickness, stroke);
    }

    /// A straight line on an earlier page, like the running header's rule.
    pub fn line_on(
        &mut self,
        page: usize,
        from: (f32, f32),
        to: (f32, f32),
        thickness: f32,
        stroke: Rgb,
    ) {
        let ops = format!(
            "{} {thickness:.2} w {:.2} {:.2} m {:.2} {:.2} l S",
            colour(stroke, "RG"),
            from.0,
            PAGE_HEIGHT - from.1,
            to.0,
            PAGE_HEIGHT - to.1
        );
        self.put(page, &ops);
    }

    /// A filled bar with round ends.
    pub fn bar(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgb) {
        let ops = format!(
            "{} {} f",
            colour(fill, "rg"),
            rounded_path(x, y, w, h, h / 2.0)
        );
        let page = self.last();
        self.put(page, &ops);
    }

    /// A filled shape through `points`, closed back to the first.
    pub fn polygon(&mut self, points: &[(f32, f32)], fill: Rgb) {
        let mut ops = colour(fill, "rg");
        for (i, (x, y)) in points.iter().enumerate() {
            let operator = if i == 0 { "m" } else { "l" };
            ops.push_str(&format!(" {x:.2} {:.2} {operator}", PAGE_HEIGHT - y));
        }
        ops.push_str(" h f");
        let page = self.last();
        self.put(page, &ops);
    }

    /// Everything drawn inside `inside` is clipped to the rounded rectangle, then outlined.
    pub fn clipped(
        &mut self,
        (x, y, w, h, radius): (f32, f32, f32, f32, f32),
        stroke: Rgb,
        inside: impl FnOnce(&mut Pdf),
    ) {
        let page = self.last();
        self.put(page, &format!("q {} W n", rounded_path(x, y, w, h, radius)));
        inside(self);
        let page = self.last();
        self.put(page, "Q");
        let ops = format!(
            "{} 0.75 w {} S",
            colour(stroke, "RG"),
            rounded_path(x, y, w, h, radius)
        );
        self.put(page, &ops);
    }

    /// Text with its baseline at `y`.
    pub fn text(&mut self, x: f32, y: f32, font: Font, size: f32, fill: Rgb, text: &str) {
        self.text_on(self.last(), x, y, font, size, fill, text);
    }

    /// Text on an earlier page (counting from 0), like the page numbers once the count is known.
    #[allow(clippy::too_many_arguments)]
    pub fn text_on(
        &mut self,
        page: usize,
        x: f32,
        y: f32,
        font: Font,
        size: f32,
        fill: Rgb,
        text: &str,
    ) {
        let mut ops = format!(
            "BT /{} {size:.2} Tf {} {x:.2} {:.2} Td (",
            font.resource(),
            colour(fill, "rg"),
            PAGE_HEIGHT - y
        )
        .into_bytes();
        for c in text.chars() {
            let byte = win_ansi(c);
            if matches!(byte, b'(' | b')' | b'\\') {
                ops.push(b'\\');
            }
            ops.push(byte);
        }
        ops.extend_from_slice(b") Tj ET\n");
        self.pages[page].extend_from_slice(&ops);
    }

    /// The finished file: a catalog, the page tree, the fonts, then each page and its content,
    /// and the table of where each object starts.
    pub fn finish(self) -> Vec<u8> {
        let fonts = Font::ALL.len();
        let first_page = 3 + fonts;
        let mut objects: Vec<Vec<u8>> = Vec::new();
        objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: Vec<String> = (0..self.pages.len())
            .map(|i| format!("{} 0 R", first_page + 2 * i))
            .collect();
        let font_refs: Vec<String> = Font::ALL
            .iter()
            .enumerate()
            .map(|(i, (_, resource, _))| format!("/{resource} {} 0 R", 3 + i))
            .collect();
        objects.push(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                 /Resources << /Font << {} >> >> >>",
                kids.join(" "),
                self.pages.len(),
                font_refs.join(" ")
            )
            .into_bytes(),
        );
        for (_, _, name) in Font::ALL {
            objects.push(
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>"
                )
                .into_bytes(),
            );
        }
        for (i, content) in self.pages.into_iter().enumerate() {
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>",
                    first_page + 2 * i + 1
                )
                .into_bytes(),
            );
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(&content);
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
        }

        let mut file = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(file.len());
            file.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            file.extend_from_slice(object);
            file.extend_from_slice(b"\nendobj\n");
        }
        let xref = file.len();
        file.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        file.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            file.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        file.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        file
    }
}

/// `r g b rg` (fill) or `RG` (stroke), each part 0 to 1.
fn colour((r, g, b): Rgb, operator: &str) -> String {
    let part = |v: u8| v as f32 / 255.0;
    format!("{:.3} {:.3} {:.3} {operator}", part(r), part(g), part(b))
}

/// The outline of a rounded rectangle: four sides and four quarter-circle curves.
fn rounded_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> String {
    // How far a Bézier's control points sit from the corner to draw a quarter circle.
    let k = radius * 0.5523;
    let (left, right) = (x, x + w);
    let (top, bottom) = (PAGE_HEIGHT - y, PAGE_HEIGHT - y - h);
    let r = radius;
    format!(
        "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c \
         {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c \
         {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c \
         {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h",
        left + r,
        top,
        right - r,
        top,
        right - r + k,
        top,
        right,
        top - r + k,
        right,
        top - r,
        right,
        bottom + r,
        right,
        bottom + r - k,
        right - r + k,
        bottom,
        right - r,
        bottom,
        left + r,
        bottom,
        left + r - k,
        bottom,
        left,
        bottom + r - k,
        left,
        bottom + r,
        left,
        top - r,
        left,
        top - r + k,
        left + r - k,
        top,
        left + r,
        top
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_text_in_the_built_in_fonts() {
        // "Hi" in Helvetica is 722 + 222 thousandths; Courier is 600 a character.
        assert!((width(Font::Sans, 10.0, "Hi") - 9.44).abs() < 0.001);
        assert_eq!(width(Font::Mono, 10.0, "aa:bb"), 30.0);
        assert!(width(Font::SansBold, 10.0, "Hi") > width(Font::Sans, 10.0, "Hi"));
    }

    #[test]
    fn cuts_and_wraps_to_a_width() {
        assert_eq!(fit(Font::Mono, 10.0, "short", 100.0), "short");
        assert_eq!(fit(Font::Mono, 10.0, "camera-hall", 36.0), "camer…");
        assert_eq!(
            wrap(Font::Mono, 10.0, "one two three four", 55.0),
            ["one two", "three", "four"]
        );
    }

    #[test]
    fn writes_the_marks_the_report_uses() {
        assert_eq!(win_ansi('·'), 0xb7);
        assert_eq!(win_ansi('—'), 0x97);
        assert_eq!(win_ansi('é'), 0xe9);
        assert_eq!(win_ansi('●'), b'?');
    }

    #[test]
    fn writes_a_file_whose_table_points_at_its_objects() {
        let mut pdf = Pdf::new();
        pdf.add_page();
        pdf.text(10.0, 20.0, Font::Sans, 9.0, (0, 0, 0), "a (b) \\ c");
        pdf.add_page();
        let file = pdf.finish();
        let text = String::from_utf8_lossy(&file);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains(r"(a \(b\) \\ c) Tj"));
        let xref: usize = text
            .split("startxref\n")
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .and_then(|line| line.parse().ok())
            .expect("a startxref offset");
        assert!(file[xref..].starts_with(b"xref"));
        let entries: Vec<usize> = text[xref..]
            .lines()
            .skip(3)
            .take_while(|line| line.ends_with(" n "))
            .map(|line| line[..10].parse().expect("an offset"))
            .collect();
        for (i, offset) in entries.iter().enumerate() {
            assert!(file[*offset..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
    }
}
