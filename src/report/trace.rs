//! The trace as a report: how far it got and where the loss starts up top, a row a hop, then charts
//! of each hop's share lost and average round trip.

use super::sheet::{self, ACCENT, COOL, Cell, Column, FG, MUTED, Sheet};
use super::speedtest::chart;
use crate::ping::target::Target;
use crate::ping::trace::{self, Trace};
use crate::ui::plain;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 8] = [
    Column::number("Hop", 0.06),
    Column::mono("Address", 0.19),
    Column::text("Name", 0.29),
    Column::number("Loss", 0.09),
    Column::number("Last", 0.0925),
    Column::number("Min", 0.0925),
    Column::number("Avg", 0.0925),
    Column::number("Max", 0.0925),
];

/// Traces `count` rounds, `wait` apart, then writes the report.
pub fn write(path: &Path, target: &Target, count: u32, wait: Duration) -> anyhow::Result<()> {
    println!("{}", plain::trace_headline(target, count, wait));
    let trace = trace::finished(target.ip, count, wait).map_err(anyhow::Error::msg)?;
    std::fs::write(path, document(&trace, target, wait))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The report's PDF file.
pub fn document(trace: &Trace, target: &Target, wait: Duration) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(&date, &time);
    let path = trace.path();
    let host_loss = match (trace.reached(), path.last()) {
        (true, Some(host)) => format!("{:.1}%", host.pings.loss()),
        _ => "—".to_string(),
    };
    let facts = [
        ("date", date),
        ("time", time),
        (
            "rounds",
            format!("{}, {} ms apart", trace.rounds(), wait.as_millis()),
        ),
        ("hops", path.len().to_string()),
        ("lost at host", host_loss),
    ];
    let title = plain::trace_verdict(trace);
    sheet.masthead("Trace", &title, &format!("to {}", target.label()), &facts);

    let rows: Vec<Vec<Cell>> = path
        .iter()
        .enumerate()
        .map(|(at, hop)| {
            // A hop that lost any in the darkest ink, as a host is in the ping report.
            let words = plain::trace_words(at, hop);
            let lost = hop.pings.lost() > 0;
            words
                .into_iter()
                .enumerate()
                .map(|(i, word)| {
                    let ink = if i == 1 || i == 2 || (i == 3 && lost) {
                        FG
                    } else {
                        MUTED
                    };
                    (word, ink)
                })
                .collect()
        })
        .collect();
    sheet.heading("Hops", 36.0);
    sheet.table(&COLUMNS, &rows);

    let numbers: Vec<String> = (1..=path.len()).map(|hop| hop.to_string()).collect();
    let numbers: Vec<&str> = numbers.iter().map(String::as_str).collect();
    let losses = path.iter().map(|hop| Some(hop.pings.loss())).collect();
    chart(
        &mut sheet,
        "Share lost at each hop",
        &numbers,
        losses,
        "%",
        COOL,
    );
    let averages = path.iter().map(|hop| hop.pings.average()).collect();
    let title = "Average round trip at each hop";
    chart(&mut sheet, title, &numbers, averages, "ms", ACCENT);

    sheet.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ping::pings::Ping;
    use crate::ping::trace::Event;
    use std::net::Ipv4Addr;

    fn traced(hops: &[&[bool]]) -> Trace {
        let mut trace = Trace::new(Ipv4Addr::new(192, 168, 1, 20));
        for (at, replies) in hops.iter().enumerate() {
            let router = match at + 1 == hops.len() {
                true => Ipv4Addr::new(192, 168, 1, 20),
                false => Ipv4Addr::new(192, 168, 1, at as u8 + 1),
            };
            for (round, back) in replies.iter().enumerate() {
                let ping = Ping {
                    host: at,
                    seq: round as u32 + 1,
                    time: "14:05:09".to_string(),
                    reply: back.then_some(1.5).ok_or_else(|| "timed out".to_string()),
                };
                trace.add(Event::Echo(ping, back.then_some(router)));
            }
        }
        trace.add(Event::Name(trace.ip, "router.example".to_string()));
        trace
    }

    #[test]
    fn writes_a_row_a_hop_and_two_charts() {
        let trace = traced(&[&[true, false], &[true, true]]);
        let target = Target {
            ip: trace.ip,
            name: None,
        };
        let file = document(&trace, &target, Duration::ZERO);
        let file = String::from_utf8_lossy(&file);
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(Hops)"));
        assert!(file.contains("(router.example)"));
        assert!(file.contains("(Share lost at each hop)"));
        assert!(file.contains("(Page 1 of"));
    }
}
