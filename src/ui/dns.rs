//! The lookup's answer, printed to the terminal and left there like dig's: a `#` line saying who
//! was asked and how it went, then a row a record (name, ttl, type, data), and the whois fields.
//! In the terminal's colours unless `--plain`, piped, or `NO_COLOR`.

use crate::dns::lookup::Lookup;
use crate::dns::whois::Whois;
use crate::ui::theme::TERMINAL;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{RData, Record};
use ratatui::backend::IntoCrossterm;
use ratatui::crossterm::style::StyledContent;
use ratatui::text::{Line, Span};
use std::io::IsTerminal;

pub fn print(lookups: &[Lookup], whois: Option<&Result<Whois, String>>, plain: bool) {
    let styled =
        !plain && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut lines = Vec::new();
    for lookup in lookups {
        lines.extend(lookup_lines(lookup));
        lines.push(Line::default());
    }
    if let Some(whois) = whois {
        lines.extend(whois_lines(whois));
        lines.push(Line::default());
    }
    lines.pop();
    for line in lines {
        match styled {
            true => {
                for span in &line.spans {
                    let style = line.style.patch(span.style).into_crossterm();
                    print!("{}", StyledContent::new(style, &span.content));
                }
                println!();
            }
            false => println!("{line}"),
        }
    }
}

/// `example.com · a · system (192.168.1.1) · udp · 12.3 ms`
pub fn headline(lookup: &Lookup) -> String {
    format!(
        "{} · {} · {} · {} · {:.1} ms",
        trimmed(&lookup.name.to_string()),
        lookup.kind.to_string().to_lowercase(),
        lookup.server.label(),
        if lookup.tcp { "tcp" } else { "udp" },
        lookup.took.as_secs_f32() * 1000.0
    )
}

/// How the answer went: `authoritative · 2 records`, `non-authoritative · no such name`, or
/// `failed: no answer in 6 s`; and whether that's good news.
pub fn verdict(lookup: &Lookup) -> (String, bool) {
    let reply = match &lookup.reply {
        Ok(reply) => reply,
        Err(why) => return (format!("failed: {why}"), false),
    };
    let authority = match reply.metadata.authoritative {
        true => "authoritative",
        false => "non-authoritative",
    };
    let outcome = match reply.metadata.response_code {
        ResponseCode::NoError => match reply.answers.len() {
            0 => format!("no {} records", lookup.kind.to_string().to_lowercase()),
            1 => "1 record".to_string(),
            count => format!("{count} records"),
        },
        ResponseCode::NXDomain => "no such name".to_string(),
        code => code.to_string().to_lowercase(),
    };
    let good = reply.metadata.response_code == ResponseCode::NoError && !reply.answers.is_empty();
    (format!("{authority} · {outcome}"), good)
}

/// A record's name, ttl, type and data as the printed rows and the report write them.
pub fn record_words(record: &Record) -> [String; 4] {
    [
        trimmed(&record.name.to_string()),
        ttl(record.ttl),
        record.record_type().to_string().to_lowercase(),
        record.data.to_string(),
    ]
}

/// Seconds as the larger unit they make: `300` is `5m`, `86400` is `1d`, `90` stays `90s`.
fn ttl(seconds: u32) -> String {
    match seconds {
        0 => "0s".to_string(),
        s if s % 86400 == 0 => format!("{}d", s / 86400),
        s if s % 3600 == 0 => format!("{}h", s / 3600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

/// A name without the root's dot at the end.
fn trimmed(name: &str) -> String {
    match name {
        "." => name.to_string(),
        _ => name.trim_end_matches('.').to_string(),
    }
}

fn lookup_lines(lookup: &Lookup) -> Vec<Line<'static>> {
    let theme = &TERMINAL;
    let (verdict, good) = verdict(lookup);
    let style = match (&lookup.reply, good) {
        (Err(_), _) => theme.warning(),
        (Ok(_), true) => theme.good(),
        (Ok(_), false) => theme.muted(),
    };
    let mut lines = vec![
        Line::styled(format!("# {}", headline(lookup)), theme.comment()),
        Line::styled(verdict, style),
    ];
    let Ok(reply) = &lookup.reply else {
        return lines;
    };
    lines.extend(record_lines(&reply.answers));
    if !reply.authorities.is_empty() {
        lines.push(Line::styled("# authority", theme.comment()));
        lines.extend(record_lines(&reply.authorities));
    }
    lines
}

/// The records lined up in columns.
fn record_lines(records: &[Record]) -> Vec<Line<'static>> {
    let theme = &TERMINAL;
    let rows: Vec<[String; 4]> = records.iter().map(record_words).collect();
    let width = |column: usize| {
        let widest = rows.iter().map(|row| row[column].chars().count()).max();
        widest.unwrap_or_default()
    };
    let (name, ttl, kind) = (width(0), width(1), width(2));
    records
        .iter()
        .zip(&rows)
        .map(|(record, [owner, seconds, record_type, data])| {
            let data_style = match record.data {
                RData::A(_) | RData::AAAA(_) => theme.address(),
                _ => theme.text(),
            };
            Line::from(vec![
                Span::styled(format!("{owner:<name$}  "), theme.text()),
                Span::styled(format!("{seconds:>ttl$}  "), theme.muted()),
                Span::styled(format!("{record_type:<kind$}  "), theme.muted()),
                Span::styled(data.clone(), data_style),
            ])
        })
        .collect()
}

fn whois_lines(whois: &Result<Whois, String>) -> Vec<Line<'static>> {
    let theme = &TERMINAL;
    let whois = match whois {
        Ok(whois) => whois,
        Err(why) => {
            return vec![
                Line::styled("# whois", theme.comment()),
                Line::styled(format!("failed: {why}"), theme.warning()),
            ];
        }
    };
    let mut lines = vec![Line::styled(
        format!("# whois {} · {}", whois.query, whois.servers.join(" · ")),
        theme.comment(),
    )];
    if whois.fields.is_empty() {
        lines.extend(
            whois
                .text
                .lines()
                .map(|line| Line::styled(line.to_string(), theme.text())),
        );
        return lines;
    }
    let width = whois.fields.iter().map(|(field, _)| field.len()).max();
    let width = width.unwrap_or_default();
    for (field, values) in &whois.fields {
        for (i, value) in values.iter().enumerate() {
            let label = if i == 0 { *field } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("{label:<width$}  "), theme.muted()),
                Span::styled(value.clone(), theme.text()),
            ]));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttls_read_in_their_largest_unit() {
        assert_eq!(ttl(300), "5m");
        assert_eq!(ttl(86400), "1d");
        assert_eq!(ttl(7200), "2h");
        assert_eq!(ttl(90), "90s");
        assert_eq!(ttl(0), "0s");
    }
}
