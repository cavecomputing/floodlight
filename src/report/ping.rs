//! The pings as a report: the average and the share lost up top, with several hosts a table of
//! their totals (each name in its line's colour), then one chart of every host's round trips. Every
//! ping is in the chart, so there is no table of them.

use super::pdf::Rgb;
use super::sheet::{self, ACCENT, AQUA, Cell, Column, FG, MUTED, Sheet};
use super::speedtest::shared_chart;
use crate::ping::pings::{self, Pings};
use crate::ping::target::Target;
use crate::ui::plain;
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 7] = [
    Column::mono("Host", 0.31),
    Column::number("Avg", 0.115),
    Column::number("Min", 0.115),
    Column::number("Max", 0.115),
    Column::number("Jitter", 0.115),
    Column::number("Sent", 0.09),
    Column::number("Lost", 0.14),
];

/// Each host's line on the chart, in the screen's order: aqua (green on paper), blue, amber,
/// purple, then the text colour.
const INKS: [Rgb; 5] = [AQUA, ACCENT, (0x9a, 0x67, 0x00), (0x82, 0x50, 0xdf), FG];

/// Pings each host `count` times, `wait` apart, saying how each ping went, then writes the
/// report.
pub fn write(path: &Path, targets: &[Target], count: u32, wait: Duration) -> anyhow::Result<()> {
    let ips: Vec<Ipv4Addr> = targets.iter().map(|target| target.ip).collect();
    let pings = pings::finished(&ips, count, wait, |ping| {
        let [seq, _, latency] = plain::ping_words(ping);
        let host = targets[ping.host].label();
        println!("{host} · ping {seq} of {count} · {latency}");
    })
    .map_err(anyhow::Error::msg)?;
    std::fs::write(path, document(&pings, targets, wait))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn ms(value: Option<f32>) -> String {
    value.map_or("—".to_string(), |ms| format!("{ms:.2} ms"))
}

/// The report's PDF file: `pings` holds each of the `targets`' pings, in the same order.
pub fn document(pings: &[Pings], targets: &[Target], wait: Duration) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(&date, &time);
    let sent: usize = pings.iter().map(|pings| pings.pings.len()).sum();
    let lost: usize = pings.iter().map(Pings::lost).sum();
    let each = pings.first().map_or(0, |pings| pings.pings.len());
    let apart = format!("{} ms apart", wait.as_millis());

    let labels: Vec<String> = targets.iter().map(Target::label).collect();
    match pings {
        [only] => {
            let title = format!("{} average · {:.1}% lost", ms(only.average()), only.loss());
            let facts = [
                ("date", date),
                ("time", time),
                ("sent", format!("{each}, {apart}")),
                ("lost", lost.to_string()),
                ("min", ms(only.min())),
                ("max", ms(only.max())),
                ("jitter", ms(only.jitter())),
            ];
            sheet.masthead("Ping", &title, &format!("to {}", labels[0]), &facts);
        }
        _ => {
            let title = format!("{} hosts · {lost} of {sent} lost", pings.len());
            let facts = [
                ("date", date),
                ("time", time),
                ("sent", format!("{each} each, {apart}")),
                ("lost", lost.to_string()),
            ];
            sheet.masthead("Ping", &title, &format!("to {}", labels.join(", ")), &facts);
        }
    }

    if targets.len() > 1 {
        let rows: Vec<Vec<Cell>> = pings
            .iter()
            .zip(&labels)
            .enumerate()
            .map(|(i, (pings, label))| {
                // A host that lost any in the darkest ink, as a failed run is in the speed
                // test's report. The name is in its line's colour: the chart's key.
                let lost = format!("{} · {:.1}%", pings.lost(), pings.loss());
                let ink = if pings.lost() > 0 { FG } else { MUTED };
                vec![
                    (label.clone(), INKS[i % INKS.len()]),
                    (ms(pings.average()), FG),
                    (ms(pings.min()), MUTED),
                    (ms(pings.max()), MUTED),
                    (ms(pings.jitter()), MUTED),
                    (pings.pings.len().to_string(), MUTED),
                    (lost, ink),
                ]
            })
            .collect();
        sheet.heading("Hosts", 36.0);
        sheet.table(&COLUMNS, &rows);
    }
    // One line a host on one chart, numbered along the bottom by the host that sent the most.
    let most = pings.iter().map(|pings| pings.pings.len()).max();
    let seqs: Vec<String> = (1..=most.unwrap_or_default())
        .map(|seq| seq.to_string())
        .collect();
    let seqs: Vec<&str> = seqs.iter().map(String::as_str).collect();
    let series: Vec<(Vec<Option<f32>>, Rgb)> = pings
        .iter()
        .enumerate()
        .map(|(i, pings)| {
            let replies = pings.pings.iter();
            let round_trips = replies.map(|ping| ping.reply.as_ref().ok().copied());
            // One host's line is the report's usual blue.
            let ink = if targets.len() == 1 {
                ACCENT
            } else {
                INKS[i % INKS.len()]
            };
            (round_trips.collect(), ink)
        })
        .collect();
    shared_chart(&mut sheet, "Round trip of each ping", &seqs, &series, "ms");

    sheet.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ping::pings::Ping;

    fn pings(replies: &[Option<f32>]) -> Pings {
        let pings = replies
            .iter()
            .enumerate()
            .map(|(i, reply)| Ping {
                host: 0,
                seq: i as u32 + 1,
                time: format!("14:05:0{i}"),
                reply: reply.ok_or_else(|| "timed out".to_string()),
            })
            .collect();
        Pings { pings }
    }

    fn target(last: u8, name: Option<&str>) -> Target {
        Target {
            ip: Ipv4Addr::new(192, 168, 1, last),
            name: name.map(str::to_string),
        }
    }

    #[test]
    fn writes_one_chart_for_every_host_and_no_table_of_pings() {
        let targets = [target(1, Some("gateway")), target(20, None)];
        let measured = [pings(&[Some(1.25), None, Some(2.5)]), pings(&[Some(0.5)])];
        let file =
            String::from_utf8_lossy(&document(&measured, &targets, Duration::ZERO)).to_string();
        assert!(file.starts_with("%PDF"));
        assert_eq!(file.matches("(Round trip of each ping)").count(), 1);
        assert!(!file.contains("(Round trip to"));
        assert!(file.contains("(Hosts)"));
        assert!(!file.contains("(Pings)"));
        assert!(file.contains("(Page 1 of"));
    }
}
