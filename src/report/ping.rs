//! The pings as a report: the average and the share lost up top, what they mean in a few
//! sentences, with several hosts a table of their totals, then a chart of each host's round trips.
//! Every ping is in the charts, so there is no table of them.

use super::sheet::{self, ACCENT, Cell, Column, FG, MUTED, Sheet};
use super::speedtest::chart;
use crate::ping::pings::{self, Pings};
use crate::ping::target::Target;
use crate::ui::plain;
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 7] = [
    Column::mono("host", 0.31),
    Column::number("sent", 0.09),
    Column::number("lost", 0.14),
    Column::number("min", 0.115),
    Column::number("avg", 0.115),
    Column::number("max", 0.115),
    Column::number("jitter", 0.115),
];

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
    let mut sheet = Sheet::new(format!("Ping · {date} {time}"));
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
    sheet.summary(&summary(pings, &labels, wait));

    if targets.len() > 1 {
        let rows: Vec<Vec<Cell>> = pings
            .iter()
            .zip(&labels)
            .map(|(pings, label)| {
                // A host that lost any in the darkest ink, as a failed run is in the speed
                // test's report.
                let lost = format!("{} · {:.1}%", pings.lost(), pings.loss());
                let ink = if pings.lost() > 0 { FG } else { MUTED };
                vec![
                    (label.clone(), FG),
                    (pings.pings.len().to_string(), MUTED),
                    (lost, ink),
                    (ms(pings.min()), MUTED),
                    (ms(pings.average()), MUTED),
                    (ms(pings.max()), MUTED),
                    (ms(pings.jitter()), MUTED),
                ]
            })
            .collect();
        sheet.heading("Hosts", 36.0);
        sheet.table(&COLUMNS, &rows);
    }
    for (pings, label) in pings.iter().zip(&labels) {
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
        let title = match targets.len() {
            1 => "Round trip of each ping".to_string(),
            _ => format!("Round trip to {label}"),
        };
        chart(&mut sheet, &title, &seqs, round_trips, "ms", ACCENT);
    }

    sheet.finish()
}

/// What the pings found, in a sentence or three a host.
fn summary(pings: &[Pings], labels: &[String], wait: Duration) -> String {
    let each = pings.first().map_or(0, |pings| pings.pings.len());
    let plural = if each == 1 { "" } else { "s" };
    let to = match labels {
        [label] => label.clone(),
        _ => format!("each of {} hosts", labels.len()),
    };
    let mut text = format!(
        "Sent {each} ping{plural} to {to}, {} ms apart, each given {} s to come back.",
        wait.as_millis(),
        pings::TIMEOUT.as_secs(),
    );
    for (pings, label) in pings.iter().zip(labels) {
        // With several hosts each one's sentences say whose they are.
        let from = match labels.len() {
            1 => String::new(),
            _ => format!("From {label}, "),
        };
        text.push_str(&host_summary(pings, &from));
    }
    text
}

/// One host's sentences: how many came back, how fast, and which were lost.
fn host_summary(pings: &Pings, from: &str) -> String {
    let sent = pings.pings.len();
    let lost = pings.lost();
    if lost == sent {
        return match from {
            "" => " None came back: the host didn't answer.".to_string(),
            _ => format!(" {from}none came back: the host didn't answer."),
        };
    }
    let mut text = String::new();
    if let (Some(min), Some(average), Some(max)) = (pings.min(), pings.average(), pings.max()) {
        text.push_str(&format!(
            " {from}{} came back, in {min:.2} to {max:.2} ms ({average:.2} ms on average)",
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
        // A handful are worth naming; past that the chart's gaps show them.
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
    fn sums_up_the_pings_and_names_the_lost() {
        let wait = Duration::from_millis(500);
        let gateway = ["gateway (192.168.1.1)".to_string()];
        assert_eq!(
            summary(&[pings(&[Some(1.0), None, Some(3.0)])], &gateway, wait),
            "Sent 3 pings to gateway (192.168.1.1), 500 ms apart, each given 2 s to come back. \
             2 came back, in 1.00 to 3.00 ms (2.00 ms on average), varying 2.00 ms from one to \
             the next (the jitter). 1 was lost (33.3%): number 2 (timed out)."
        );
        assert_eq!(
            summary(&[pings(&[None, None])], &gateway, wait),
            "Sent 2 pings to gateway (192.168.1.1), 500 ms apart, each given 2 s to come back. \
             None came back: the host didn't answer."
        );
    }

    #[test]
    fn sums_up_each_host() {
        let labels = ["192.168.1.1".to_string(), "192.168.1.2".to_string()];
        assert_eq!(
            summary(
                &[pings(&[Some(1.0), Some(2.0)]), pings(&[None, None])],
                &labels,
                Duration::from_millis(250)
            ),
            "Sent 2 pings to each of 2 hosts, 250 ms apart, each given 2 s to come back. From \
             192.168.1.1, 2 came back, in 1.00 to 2.00 ms (1.50 ms on average), varying 1.00 ms \
             from one to the next (the jitter). None was lost. From 192.168.1.2, none came back: \
             the host didn't answer."
        );
    }

    #[test]
    fn writes_a_chart_a_host_and_no_table_of_pings() {
        let targets = [target(1, Some("gateway")), target(20, None)];
        let measured = [pings(&[Some(1.25), None, Some(2.5)]), pings(&[Some(0.5)])];
        let file =
            String::from_utf8_lossy(&document(&measured, &targets, Duration::ZERO)).to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(ROUND TRIP TO 192.168.1.20)"));
        assert!(file.contains("(HOSTS)"));
        assert!(!file.contains("(PINGS)"));
        assert!(file.contains("(page 1 of"));
    }
}
