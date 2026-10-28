//! The subnet worked out, printed to the terminal and left there: a row a value, its label muted
//! and the addresses in the address colour. Plain under `--plain`, a pipe or `NO_COLOR`.

use crate::calc::Subnet;
use crate::ui::theme::TERMINAL;
use ratatui::text::{Line, Span};

pub fn print(subnet: &Subnet, plain: bool) {
    let theme = &TERMINAL;
    let rows = subnet.rows();
    let width = rows.iter().map(|(label, _)| label.len()).max();
    let width = width.unwrap_or_default();
    let lines = rows
        .into_iter()
        .map(|(label, value)| {
            let style = match label {
                "address" | "network" | "broadcast" | "first" | "last" => theme.address(),
                _ => theme.text(),
            };
            Line::from(vec![
                Span::styled(format!("{label:<width$}  "), theme.muted()),
                Span::styled(value, style),
            ])
        })
        .collect();
    crate::ui::print_lines(lines, plain);
}
