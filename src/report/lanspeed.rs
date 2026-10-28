//! The LAN speed test as a report: the throughput each way up top, what it means in a few
//! sentences, a chart a direction (of the intervals for one run, of the runs for several), and a
//! table of every run.

use super::sheet::{self, AQUA, COOL, Cell, Column, FG, MUTED, SUBTLE, Sheet};
use super::speedtest::chart;
use crate::lanspeed::runs::{self, Event, Run, Runs, Settings};
use crate::lanspeed::wire::{Direction, Protocol, Report};
use crate::ui::plain;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 6] = [
    Column {
        label: "time",
        width: 0.14,
        mono: true,
        right: false,
    },
    Column {
        label: "upload",
        width: 0.18,
        mono: false,
        right: true,
    },
    Column {
        label: "download",
        width: 0.18,
        mono: false,
        right: true,
    },
    Column {
        label: "jitter",
        width: 0.15,
        mono: false,
        right: true,
    },
    Column {
        label: "lost",
        width: 0.15,
        mono: false,
        right: true,
    },
    Column {
        label: "",
        width: 0.20,
        mono: false,
        right: false,
    },
];

/// Runs the test `count` times, `wait` apart, saying how each transfer went, then writes the
/// report.
pub fn write(path: &Path, settings: Settings, count: u32, wait: Duration) -> anyhow::Result<()> {
    let runs = runs::finished(settings.clone(), count, wait, |event| match event {
        Event::Total(direction, report) => {
            println!(
                "{}",
                plain::lanspeed_row(*direction, report, true).join(" · ")
            );
        }
        Event::Failed(why) => println!("failed: {why}"),
        _ => {}
    });
    if runs
        .runs
        .iter()
        .all(|run| run.upload.is_none() && run.download.is_none())
    {
        anyhow::bail!("no run measured anything, so there is nothing to report");
    }
    std::fs::write(path, document(&runs, &settings, wait))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn mbps(value: Option<f32>) -> String {
    value.map_or("—".to_string(), |mbps| format!("{mbps:.1} Mbps"))
}

/// The report's PDF file.
pub fn document(runs: &Runs, settings: &Settings, wait: Duration) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(format!("LAN speed · {date} {time}"));
    let udp = settings.protocol == Protocol::Udp;
    let up = runs.median(Run::upload_mbps);
    let down = runs.median(Run::download_mbps);
    let title = match (up, down) {
        (Some(_), Some(_)) => format!("{} up · {} down", mbps(up), mbps(down)),
        (Some(_), None) => format!("{} up", mbps(up)),
        (None, Some(_)) => format!("{} down", mbps(down)),
        (None, None) => "nothing measured".to_string(),
    };
    let subtitle = format!(
        "to {} · {} · {} stream{} · {} s each way",
        settings.host,
        settings.protocol.name(),
        settings.streams,
        if settings.streams == 1 { "" } else { "s" },
        settings.seconds.as_secs_f32()
    );
    let mut facts = vec![("date", date), ("time", time)];
    if udp {
        let rate = match settings.mbps {
            0.0 => "as fast as it can".to_string(),
            rate => format!("{rate} Mbps"),
        };
        facts.push(("sent at", rate));
        let jitter = runs.median(Run::jitter);
        facts.push((
            "jitter",
            jitter.map_or("—".to_string(), |ms| format!("{ms:.2} ms")),
        ));
        let loss = runs.median(Run::loss);
        facts.push((
            "lost",
            loss.map_or("—".to_string(), |share| format!("{share:.1}%")),
        ));
    }
    if runs.runs.len() > 1 {
        let apart = format!("{}, {} s apart", runs.runs.len(), wait.as_secs());
        facts.push(("runs", apart));
    }
    sheet.masthead("LAN speed", &title, &subtitle, &facts);
    sheet.summary(&summary(runs, settings, wait));

    for direction in &settings.directions {
        let ink = match direction {
            Direction::Upload => COOL,
            Direction::Download => AQUA,
        };
        match runs.runs.as_slice() {
            // One run: each interval of the transfer.
            [run] => {
                let intervals: Vec<&Report> = run
                    .intervals
                    .iter()
                    .filter(|(way, _)| way == direction)
                    .map(|(_, report)| report)
                    .collect();
                let ends: Vec<String> = intervals
                    .iter()
                    .map(|report| format!("{:.0} s", report.to))
                    .collect();
                let ends: Vec<&str> = ends.iter().map(String::as_str).collect();
                let values = intervals.iter().map(|report| Some(report.mbps())).collect();
                let title = format!("{} through the transfer", capital(direction.name()));
                chart(&mut sheet, &title, &ends, values, "Mbps", ink);
            }
            // Several: each run's total.
            _ => {
                let times: Vec<&str> = runs.runs.iter().map(|run| run.time.as_str()).collect();
                let values = runs.runs.iter().map(|run| run.mbps(*direction)).collect();
                let title = format!("{} across runs", capital(direction.name()));
                chart(&mut sheet, &title, &times, values, "Mbps", ink);
            }
        }
    }

    let rows: Vec<Vec<Cell>> = runs
        .runs
        .iter()
        .map(|run| {
            let ink = |words: &str, ink| if words == "—" { SUBTLE } else { ink };
            let up = mbps(run.upload_mbps());
            let down = mbps(run.download_mbps());
            let jitter = run
                .jitter()
                .map_or("—".to_string(), |ms| format!("{ms:.2} ms"));
            let lost = run
                .loss()
                .map_or("—".to_string(), |share| format!("{share:.1}%"));
            let note = run
                .failed
                .as_ref()
                .map_or(String::new(), |why| format!("failed: {why}"));
            vec![
                (run.time.clone(), MUTED),
                (up.clone(), ink(&up, COOL)),
                (down.clone(), ink(&down, AQUA)),
                (jitter.clone(), ink(&jitter, MUTED)),
                (lost.clone(), ink(&lost, MUTED)),
                (note, FG),
            ]
        })
        .collect();
    sheet.heading("Runs", 36.0);
    sheet.table(&COLUMNS, &rows);
    sheet.finish()
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// What the runs measured, in a sentence or three.
fn summary(runs: &Runs, settings: &Settings, wait: Duration) -> String {
    let measured: Vec<&Run> = runs
        .runs
        .iter()
        .filter(|run| run.failed.is_none())
        .collect();
    let streams = match settings.streams {
        1 => "one connection".to_string(),
        n => format!("{n} connections at once"),
    };
    let how = match settings.protocol {
        Protocol::Tcp => format!("over TCP on {streams}"),
        Protocol::Udp => match settings.mbps {
            0.0 => format!("over UDP on {streams}, sending as fast as it could"),
            mbps => format!("over UDP on {streams}, sending at {mbps} Mbps"),
        },
    };
    let each_way = |run: &Run| {
        let mut parts = Vec::new();
        if let Some(up) = run.upload_mbps() {
            parts.push(format!("{up:.1} Mbps sending to it"));
        }
        if let Some(down) = run.download_mbps() {
            parts.push(format!("{down:.1} Mbps receiving from it"));
        }
        parts.join(" and ")
    };
    let mut text = match measured.as_slice() {
        [] => format!("No run measured anything against {} {how}.", settings.host),
        [run] => format!(
            "The link to {} carried {} {how}, each way for {} seconds.",
            settings.host,
            each_way(run),
            settings.seconds.as_secs_f32()
        ),
        _ => {
            let median = |result: fn(&Run) -> Option<f32>| runs.median(result);
            let mut parts = Vec::new();
            if let Some(up) = median(Run::upload_mbps) {
                parts.push(format!(
                    "sending went from {} (median {up:.1})",
                    range(&measured, Run::upload_mbps)
                ));
            }
            if let Some(down) = median(Run::download_mbps) {
                parts.push(format!(
                    "receiving from {} (median {down:.1})",
                    range(&measured, Run::download_mbps)
                ));
            }
            format!(
                "Over {} runs {} s apart against {} {how}, {}.",
                measured.len(),
                wait.as_secs(),
                settings.host,
                parts.join(" and ")
            )
        }
    };
    if settings.protocol == Protocol::Udp
        && let (Some(loss), Some(jitter)) = (runs.median(Run::loss), runs.median(Run::jitter))
    {
        text.push_str(&format!(
            " {loss:.1}% of the datagrams were lost on the way, and one arrived {jitter:.2} ms \
             earlier or later than the one before, on average (the jitter)."
        ));
    }
    for run in runs.runs.iter().filter(|run| run.failed.is_some()) {
        let why = run.failed.as_deref().unwrap_or_default();
        text.push_str(&format!(" The run at {} failed: {why}.", run.time));
    }
    text
}

/// `210.4 to 250.3 Mbps`: the slowest and fastest of one result across the runs.
fn range(runs: &[&Run], result: fn(&Run) -> Option<f32>) -> String {
    let values: Vec<f32> = runs.iter().filter_map(|run| result(run)).collect();
    let slowest = values.iter().copied().fold(f32::MAX, f32::min);
    let fastest = values.iter().copied().fold(0.0, f32::max);
    format!("{slowest:.1} to {fastest:.1} Mbps")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanspeed::wire::{Datagrams, Report};
    use std::net::SocketAddr;

    fn settings(protocol: Protocol) -> Settings {
        Settings {
            host: "192.168.1.20".to_string(),
            server: SocketAddr::from(([192, 168, 1, 20], 5201)),
            protocol,
            directions: vec![Direction::Upload, Direction::Download],
            streams: 1,
            seconds: Duration::from_secs(10),
            interval: Duration::from_secs(1),
            mbps: 100.0,
        }
    }

    fn report(mbps: f32, udp: bool) -> Report {
        Report {
            from: 0.0,
            to: 1.0,
            bytes: (mbps * 1e6 / 8.0) as u64,
            datagrams: udp.then_some(Datagrams {
                got: 990,
                lost: 10,
                jitter_ms: 0.25,
            }),
        }
    }

    fn measured(uploads: &[f32], udp: bool) -> Runs {
        let mut runs = Runs::default();
        for (i, up) in uploads.iter().enumerate() {
            runs.apply(Event::Started);
            runs.runs[i].time = format!("14:0{i}:00");
            runs.apply(Event::Interval(Direction::Upload, report(*up, udp)));
            runs.apply(Event::Total(Direction::Upload, report(*up, udp)));
            runs.apply(Event::Interval(Direction::Download, report(up * 2.0, udp)));
            runs.apply(Event::Total(Direction::Download, report(up * 2.0, udp)));
            runs.apply(Event::Finished);
        }
        runs
    }

    #[test]
    fn sums_up_one_run() {
        assert_eq!(
            summary(
                &measured(&[400.0], false),
                &settings(Protocol::Tcp),
                Duration::ZERO
            ),
            "The link to 192.168.1.20 carried 400.0 Mbps sending to it and 800.0 Mbps receiving \
             from it over TCP on one connection, each way for 10 seconds."
        );
    }

    #[test]
    fn sums_up_many_udp_runs_and_the_ones_that_failed() {
        let mut runs = measured(&[90.0, 95.0, 92.0], true);
        runs.apply(Event::Started);
        runs.runs[3].time = "14:03:00".to_string();
        runs.apply(Event::Failed(
            "the server is busy with another test".to_string(),
        ));
        assert_eq!(
            summary(&runs, &settings(Protocol::Udp), Duration::from_secs(30)),
            "Over 3 runs 30 s apart against 192.168.1.20 over UDP on one connection, sending at \
             100 Mbps, sending went from 90.0 to 95.0 Mbps (median 92.0) and receiving from \
             180.0 to 190.0 Mbps (median 184.0). 1.0% of the datagrams were lost on the way, \
             and one arrived 0.25 ms earlier or later than the one before, on average (the \
             jitter). The run at 14:03:00 failed: the server is busy with another test."
        );
    }

    #[test]
    fn writes_a_report_with_a_chart_a_direction() {
        let file = String::from_utf8_lossy(&document(
            &measured(&[400.0], false),
            &settings(Protocol::Tcp),
            Duration::ZERO,
        ))
        .to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("400.0 Mbps up"));
        assert!(file.contains("(UPLOAD THROUGH THE TRANSFER)"));
        assert!(file.contains("(DOWNLOAD THROUGH THE TRANSFER)"));
        assert!(file.contains("(RUNS)"));
        let several = String::from_utf8_lossy(&document(
            &measured(&[400.0, 410.0], false),
            &settings(Protocol::Tcp),
            Duration::from_secs(5),
        ))
        .to_string();
        assert!(several.contains("(UPLOAD ACROSS RUNS)"));
    }
}
