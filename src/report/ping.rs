//! The pings as a report: the average and the share lost up top, what they mean in a few
//! sentences, a chart of every round trip, and a table of every ping.

use super::sheet::{self, ACCENT, Cell, Column, MUTED, SUBTLE, Sheet};
use super::speedtest::chart;
use crate::ping::pings::{self, Pings};
use crate::ping::target::Target;
use crate::ui::plain;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 3] = [
    Column {
        label: "seq",
        width: 0.15,
        mono: true,
        right: false,
    },
    Column {
        label: "time",
        width: 0.2,
        mono: true,
        right: false,
    },
    Column {
        label: "latency",
        width: 0.65,
        mono: false,
        right: false,
    },
];

/// Pings `count` times, `wait` apart, saying how each went, then writes the report.
pub fn write(path: &Path, target: &Target, count: u32, wait: Duration) -> anyhow::Result<()> {
    let pings = pings::finished(target.ip, count, wait, |ping| {
        let [seq, _, latency] = plain::ping_words(ping);
        println!("ping {seq} of {count} · {latency}");
    })
    .map_err(anyhow::Error::msg)?;
    std::fs::write(path, document(&pings, target, wait))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The report's PDF file.
pub fn document(pings: &Pings, target: &Target, wait: Duration) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(format!("Ping · {date} {time}"));
    let ms = |value: Option<f32>| value.map_or("—".to_string(), |ms| format!("{ms:.2} ms"));

    let title = format!(
        "{} average · {:.1}% lost",
        ms(pings.average()),
        pings.loss()
    );
    let facts = [
        ("date", date),
        ("time", time),
        (
            "sent",
            format!("{}, {} ms apart", pings.pings.len(), wait.as_millis()),
        ),
        ("lost", pings.lost().to_string()),
        ("min", ms(pings.min())),
        ("max", ms(pings.max())),
        ("jitter", ms(pings.jitter())),
    ];
    sheet.masthead("Ping", &title, &format!("to {}", target.label()), &facts);
    sheet.summary(&summary(pings, target, wait));

    let seqs: Vec<String> = pings
        .pings
        .iter()
        .map(|ping| ping.seq.to_string())
        .collect();
    let seqs: Vec<&str> = seqs.iter().map(String::as_str).collect();
    let round_trips = pings
        .pings
        .iter()
        .map(|ping| ping.reply.as_ref().ok().copied())
        .collect();
    chart(
        &mut sheet,
        "Round trip of each ping",
        &seqs,
        round_trips,
        "ms",
        ACCENT,
    );

    let rows: Vec<Vec<Cell>> = pings
        .pings
        .iter()
        .map(|ping| {
            let [seq, time, latency] = plain::ping_words(ping);
            // A lost ping in the darkest ink, as a failed run is in the speed test's report.
            let ink = if ping.reply.is_ok() { MUTED } else { sheet::FG };
            vec![(seq, SUBTLE), (time, MUTED), (latency, ink)]
        })
        .collect();
    sheet.heading("Pings", 36.0);
    sheet.table(&COLUMNS, &rows);
    sheet.finish()
}

/// What the pings found, in a sentence or three.
fn summary(pings: &Pings, target: &Target, wait: Duration) -> String {
    let sent = pings.pings.len();
    let lost = pings.lost();
    let mut text = format!(
        "Sent {sent} ping{} to {}, {} ms apart, each given {} s to come back.",
        if sent == 1 { "" } else { "s" },
        target.label(),
        wait.as_millis(),
        pings::TIMEOUT.as_secs(),
    );
    if let (Some(min), Some(average), Some(max)) = (pings.min(), pings.average(), pings.max()) {
        text.push_str(&format!(
            " {} came back, in {min:.2} to {max:.2} ms ({average:.2} ms on average)",
            sent - lost
        ));
        match pings.jitter() {
            Some(jitter) => text.push_str(&format!(
                ", varying {jitter:.2} ms from one to the next (the jitter)."
            )),
            None => text.push('.'),
        }
    }
    let lost_ones: Vec<String> = pings
        .pings
        .iter()
        .filter_map(|ping| {
            ping.reply
                .as_ref()
                .err()
                .map(|why| format!("{} ({why})", ping.seq))
        })
        .collect();
    match lost_ones.len() {
        0 => text.push_str(" None was lost."),
        count if count == sent => text.push_str(" None came back: the host didn't answer."),
        // A handful are worth naming; past that the table names them.
        count if count <= 10 => text.push_str(&format!(
            " {count} {} lost ({:.1}%): {} {}.",
            if count == 1 { "was" } else { "were" },
            pings.loss(),
            if count == 1 { "number" } else { "numbers" },
            lost_ones.join(", ")
        )),
        count => text.push_str(&format!(" {count} were lost ({:.1}%).", pings.loss())),
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ping::pings::Ping;
    use std::net::Ipv4Addr;

    fn pings(replies: &[Option<f32>]) -> Pings {
        let pings = replies
            .iter()
            .enumerate()
            .map(|(i, reply)| Ping {
                seq: i as u32 + 1,
                time: format!("14:05:0{i}"),
                reply: reply.ok_or_else(|| "timed out".to_string()),
            })
            .collect();
        Pings { pings }
    }

    fn gateway() -> Target {
        Target {
            ip: Ipv4Addr::new(192, 168, 1, 1),
            name: Some("gateway".to_string()),
        }
    }

    #[test]
    fn sums_up_the_pings_and_names_the_lost() {
        let wait = Duration::from_millis(500);
        assert_eq!(
            summary(&pings(&[Some(1.0), None, Some(3.0)]), &gateway(), wait),
            "Sent 3 pings to gateway (192.168.1.1), 500 ms apart, each given 2 s to come back. \
             2 came back, in 1.00 to 3.00 ms (2.00 ms on average), varying 2.00 ms from one to \
             the next (the jitter). 1 was lost (33.3%): number 2 (timed out)."
        );
        assert_eq!(
            summary(&pings(&[None, None]), &gateway(), wait),
            "Sent 2 pings to gateway (192.168.1.1), 500 ms apart, each given 2 s to come back. \
             None came back: the host didn't answer."
        );
    }

    #[test]
    fn writes_a_report_of_every_ping() {
        let pings = pings(&[Some(1.25), None, Some(2.5)]);
        let file =
            String::from_utf8_lossy(&document(&pings, &gateway(), Duration::ZERO)).to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(timed out)"));
        assert!(file.contains("(page 1 of 1)"));
    }
}
