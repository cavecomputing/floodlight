//! The public addresses, printed to the terminal and left there like `calc`'s rows: the label
//! muted, the addresses in the address colour, a family that didn't answer saying why. Plain
//! under `--plain`, a pipe or `NO_COLOR`.

use crate::ui::theme::TERMINAL;
use crate::wanip::Wan;
use ratatui::text::{Line, Span};

pub fn print(wan: &Wan, plain: bool) {
    let theme = &TERMINAL;
    let mut rows = Vec::new();
    for (label, address) in [("ipv4", &wan.v4), ("ipv6", &wan.v6)] {
        rows.push(match address {
            Ok(address) => (label, address.to_string(), theme.address()),
            Err(why) => (label, format!("none · {why}"), theme.comment()),
        });
    }
    if let Some(isp) = &wan.isp {
        rows.push(("provider", isp.clone(), theme.text()));
    }
    if let Some(place) = &wan.place {
        rows.push(("place", place.clone(), theme.text()));
    }
    let lines = rows
        .into_iter()
        .map(|(label, value, style)| {
            Line::from(vec![
                Span::styled(format!("{label:<8}  "), theme.muted()),
                Span::styled(value, style),
            ])
        })
        .collect();
    crate::ui::print_lines(lines, plain);
}
