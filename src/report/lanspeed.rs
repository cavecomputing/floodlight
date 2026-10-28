//! The LAN speed test as a report: the throughput each way up top, a chart a direction (of the
//! intervals for one run, of the runs for several), and a table of every run.

use super::sheet::{self, AQUA, COOL, Cell, Column, FG, MUTED, SUBTLE, Sheet};
use super::speedtest::chart;
use crate::lanspeed::runs::{self, Event, Run, Runs, Settings};
use crate::lanspeed::wire::{Direction, Protocol, Report};
use crate::ui::plain;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 6] = [
    Column::mono("Time", 0.14),
    Column::number("Upload", 0.18),
    Column::number("Download", 0.18),
    Column::number("Jitter", 0.15),
    Column::number("Lost", 0.15),
    Column::text("", 0.20),
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

/// The report's PDF file.
pub fn document(runs: &Runs, settings: &Settings, wait: Duration) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(&date, &time);
    let udp = settings.protocol == Protocol::Udp;
    let up = runs.median(Run::upload_mbps);
    let down = runs.median(Run::download_mbps);
    let title = match (up, down) {
        (Some(_), Some(_)) => format!("{} up · {} down", plain::mbps(up), plain::mbps(down)),
        (Some(_), None) => format!("{} up", plain::mbps(up)),
        (None, Some(_)) => format!("{} down", plain::mbps(down)),
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
                let title = format!("{} through the transfer", titled(*direction));
                chart(&mut sheet, &title, &ends, values, "Mbps", ink);
            }
            // Several: each run's total.
            _ => {
                let times: Vec<&str> = runs.runs.iter().map(|run| run.time.as_str()).collect();
                let values = runs.runs.iter().map(|run| run.mbps(*direction)).collect();
                let title = format!("{} across runs", titled(*direction));
                chart(&mut sheet, &title, &times, values, "Mbps", ink);
            }
        }
    }

    let rows: Vec<Vec<Cell>> = runs
        .runs
        .iter()
        .map(|run| {
            let ink = |words: &str, ink| if words == "—" { SUBTLE } else { ink };
            let up = plain::mbps(run.upload_mbps());
            let down = plain::mbps(run.download_mbps());
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

/// The direction as a chart's title starts it.
fn titled(direction: Direction) -> &'static str {
    match direction {
        Direction::Upload => "Upload",
        Direction::Download => "Download",
    }
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
    fn writes_a_report_with_a_chart_a_direction() {
        let file = String::from_utf8_lossy(&document(
            &measured(&[400.0], false),
            &settings(Protocol::Tcp),
            Duration::ZERO,
        ))
        .to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("400.0 Mbps up"));
        assert!(file.contains("(Upload through the transfer)"));
        assert!(file.contains("(Download through the transfer)"));
        assert!(file.contains("(Runs)"));
        let several = String::from_utf8_lossy(&document(
            &measured(&[400.0, 410.0], false),
            &settings(Protocol::Tcp),
            Duration::from_secs(5),
        ))
        .to_string();
        assert!(several.contains("(Upload across runs)"));
    }
}
