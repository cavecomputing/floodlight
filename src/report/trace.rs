//! The trace as a report: how far it got and where the loss starts up top, what that means in a
//! few sentences, a row a hop, then charts of each hop's share lost and average round trip.

use super::sheet::{self, ACCENT, COOL, Cell, Column, FG, MUTED, Sheet};
use super::speedtest::chart;
use crate::ping::pings::TIMEOUT;
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
    sheet.summary(&summary(trace, &target.label(), wait));

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

/// What the trace found, in a few sentences.
fn summary(trace: &Trace, label: &str, wait: Duration) -> String {
    let rounds = trace.rounds();
    let plural = if rounds == 1 { "" } else { "s" };
    let path = trace.path();
    let mut text = format!(
        "Sent {rounds} round{plural} of echoes to {label}, {} ms apart, each echo set to run out \
         at one hop on the way so the router there answers, and given {} s to.",
        wait.as_millis(),
        TIMEOUT.as_secs(),
    );
    if !trace.reached() {
        text.push_str(&match path.len() {
            0 => " Nothing answered: not even the first router.".to_string(),
            hops => format!(
                " The host never answered; the last router heard was at hop {hops}. Past it the \
                 traffic is dropped, or the routers and the host don't answer echoes."
            ),
        });
        return text;
    }
    let hops = path.len();
    text.push_str(&format!(
        " It got there in {hops} hop{}.",
        if hops == 1 { "" } else { "s" }
    ));
    match (trace.loss_starts(), path.last()) {
        (Some(at), Some(host)) => text.push_str(&format!(
            " {:.1}% of the echoes to the host were lost, and the loss starts at hop {at}: every \
             hop from there on lost some, so the trouble is at that hop or the link into it.",
            host.pings.loss()
        )),
        _ => text.push_str(" None of the echoes to the host were lost."),
    }
    // Routers that lose echoes when the hops after them don't.
    let before_the_loss = |at: usize| trace.loss_starts().is_none_or(|start| at + 1 < start);
    let slow: Vec<String> = path
        .iter()
        .enumerate()
        .take(hops - 1)
        .filter(|(at, hop)| hop.pings.lost() > 0 && before_the_loss(*at))
        .map(|(at, _)| (at + 1).to_string())
        .collect();
    if !slow.is_empty() {
        text.push_str(&format!(
            " Hop{} {} lost some without the hops after {} losing any: routers answer for \
             themselves last, when busy, so that isn't traffic lost.",
            if slow.len() == 1 { "" } else { "s" },
            match slow.split_last() {
                Some((last, [])) => last.clone(),
                Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
                None => String::new(),
            },
            if slow.len() == 1 { "it" } else { "them" },
        ));
    }
    text
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
    fn says_where_the_loss_starts_and_which_routers_are_only_slow() {
        let wait = Duration::from_millis(1000);
        let trace = traced(&[
            &[true, false],
            &[true, true],
            &[true, false],
            &[false, true],
        ]);
        assert_eq!(
            summary(&trace, "192.168.1.20", wait),
            "Sent 2 rounds of echoes to 192.168.1.20, 1000 ms apart, each echo set to run out at \
             one hop on the way so the router there answers, and given 2 s to. It got there in 4 \
             hops. 50.0% of the echoes to the host were lost, and the loss starts at hop 3: every \
             hop from there on lost some, so the trouble is at that hop or the link into it. Hop \
             1 lost some without the hops after it losing any: routers answer for themselves \
             last, when busy, so that isn't traffic lost."
        );
        let trace = traced(&[&[true, false], &[false, true], &[true, true]]);
        assert!(summary(&trace, "192.168.1.20", wait).ends_with(
            "hops. None of the echoes to the host were lost. Hops 1 and 2 lost some without \
             the hops after them losing any: routers answer for themselves last, when busy, so \
             that isn't traffic lost."
        ));
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
