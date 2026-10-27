//! The speed test as a report: download and upload up top, what they mean in a few sentences,
//! and a table of every run.

use super::sheet::{self, AQUA, COOL, Cell, Column, FG, MUTED, SUBTLE, Sheet};
use crate::speedtest::runs::{self, Run, Runs};
use crate::ui::plain;
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;

const COLUMNS: [Column; 6] = [
    Column {
        label: "time",
        width: 0.12,
        mono: true,
        right: false,
    },
    Column {
        label: "latency",
        width: 0.12,
        mono: false,
        right: true,
    },
    Column {
        label: "jitter",
        width: 0.11,
        mono: false,
        right: true,
    },
    Column {
        label: "download",
        width: 0.15,
        mono: false,
        right: true,
    },
    Column {
        label: "upload",
        width: 0.15,
        mono: false,
        right: true,
    },
    Column {
        label: "server",
        width: 0.35,
        mono: false,
        right: false,
    },
];

/// Runs the test `count` times, `wait` apart, saying how each went, then writes the report.
pub fn write(runtime: &Runtime, path: &Path, count: u32, wait: Duration) -> anyhow::Result<()> {
    let mut ended = 0;
    let runs = runs::finished(runtime, count, wait, |run| {
        ended += 1;
        let [_, latency, _, download, upload, server] = plain::run_words(run);
        match &run.failed {
            Some(_) => println!("run {ended} of {count} · {server}"),
            None => println!("run {ended} of {count} · {download} down · {upload} up · {latency}"),
        }
    });
    if runs.median(Run::download).is_none() {
        anyhow::bail!("no run measured anything, so there is nothing to report");
    }
    std::fs::write(path, document(&runs, wait))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The report's PDF file.
pub fn document(runs: &Runs, wait: Duration) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(format!("Speed test · {date} {time}"));
    let mbps =
        |value: Option<f32>| value.map_or("—".to_string(), |mbps| format!("{mbps:.1} Mbps"));
    let ms = |value: Option<f32>| value.map_or("—".to_string(), |ms| format!("{ms:.1} ms"));

    let title = format!(
        "{} down · {} up",
        mbps(runs.median(Run::download)),
        mbps(runs.median(Run::upload))
    );
    let subtitle = match runs.server() {
        Some(server) => {
            let mut words = format!("against Cloudflare {}", server.place());
            if let Some(isp) = &server.isp {
                words.push_str(&format!(" · through {isp}"));
            }
            words
        }
        None => "against Cloudflare".to_string(),
    };
    let mut facts = vec![
        ("date", date),
        ("time", time),
        ("latency", ms(runs.median(Run::latency))),
        ("jitter", ms(runs.median(Run::jitter))),
    ];
    if runs.runs.len() > 1 {
        let apart = format!("{}, {} s apart", runs.runs.len(), wait.as_secs());
        facts.push(("runs", apart));
    }
    sheet.masthead("Speed test", &title, &subtitle, &facts);
    sheet.summary(&summary(runs, wait));

    let rows: Vec<Vec<Cell>> = runs
        .runs
        .iter()
        .map(|run| {
            let [time, latency, jitter, download, upload, server] = plain::run_words(run);
            // Anything the run didn't measure is in the comment colour, like on screen.
            let ink = |words: &str, ink| if words == "—" { SUBTLE } else { ink };
            vec![
                (time, MUTED),
                (latency.clone(), ink(&latency, MUTED)),
                (jitter.clone(), ink(&jitter, MUTED)),
                (download.clone(), ink(&download, AQUA)),
                (upload.clone(), ink(&upload, AQUA)),
                (server, if run.failed.is_some() { FG } else { COOL }),
            ]
        })
        .collect();
    sheet.heading("Runs", 36.0);
    sheet.table(&COLUMNS, &rows);
    sheet.finish()
}

/// What the runs measured, in a sentence or three.
fn summary(runs: &Runs, wait: Duration) -> String {
    let place = match runs.server() {
        Some(server) => match &server.city {
            Some(city) => format!("Cloudflare's data center in {city} ({})", server.colo),
            None => format!("Cloudflare's data center {}", server.colo),
        },
        None => "Cloudflare".to_string(),
    };
    let measured: Vec<&Run> = runs
        .runs
        .iter()
        .filter(|run| run.failed.is_none())
        .collect();
    let median = |result: fn(&Run) -> Option<f32>| runs.median(result).unwrap_or_default();
    let mut text = match measured.as_slice() {
        [] => "No run measured anything.".to_string(),
        [run] => format!(
            "Downloads reached {:.1} Mbps and uploads {:.1} Mbps against {place}. A request took \
             {:.1} ms to come back, give or take {:.1} ms from one to the next (the jitter).",
            run.download().unwrap_or_default(),
            run.upload().unwrap_or_default(),
            run.latency().unwrap_or_default(),
            run.jitter().unwrap_or_default(),
        ),
        _ => format!(
            "Over {} runs {} s apart against {place}, downloads went from {} (median {:.1}) and \
             uploads from {} (median {:.1}). The median request took {:.1} ms to come back, give \
             or take {:.1} ms from one to the next (the jitter).",
            measured.len(),
            wait.as_secs(),
            range(&measured, Run::download),
            median(Run::download),
            range(&measured, Run::upload),
            median(Run::upload),
            median(Run::latency),
            median(Run::jitter),
        ),
    };
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
    use crate::speedtest::cloudflare::Server;
    use crate::speedtest::runs::Event;

    fn measured(downloads: &[f32]) -> Runs {
        let mut runs = Runs::default();
        for (i, download) in downloads.iter().enumerate() {
            runs.apply(Event::Started);
            runs.apply(Event::Server(Server {
                colo: "LAX".to_string(),
                city: Some("Los Angeles".to_string()),
                isp: Some("Example Fiber".to_string()),
            }));
            for ping in [10.0, 12.0, 11.0] {
                runs.apply(Event::Ping(ping));
            }
            runs.apply(Event::Download(*download));
            runs.apply(Event::Upload(download / 10.0));
            runs.runs[i].time = format!("14:0{i}:00");
            runs.apply(Event::Finished);
        }
        runs
    }

    #[test]
    fn sums_up_one_run() {
        assert_eq!(
            summary(&measured(&[240.0]), Duration::from_secs(5)),
            "Downloads reached 240.0 Mbps and uploads 24.0 Mbps against Cloudflare's data center \
             in Los Angeles (LAX). A request took 11.0 ms to come back, give or take 1.5 ms from \
             one to the next (the jitter)."
        );
    }

    #[test]
    fn sums_up_many_runs_and_the_ones_that_failed() {
        let mut runs = measured(&[200.0, 250.0, 240.0]);
        runs.apply(Event::Started);
        runs.runs[3].time = "14:03:00".to_string();
        runs.apply(Event::Failed("cloudflare answered 429".to_string()));
        assert_eq!(
            summary(&runs, Duration::from_secs(30)),
            "Over 3 runs 30 s apart against Cloudflare's data center in Los Angeles (LAX), \
             downloads went from 200.0 to 250.0 Mbps (median 240.0) and uploads from 20.0 to \
             25.0 Mbps (median 24.0). The median request took 11.0 ms to come back, give or take \
             1.5 ms from one to the next (the jitter). The run at 14:03:00 failed: cloudflare \
             answered 429."
        );
    }

    #[test]
    fn writes_a_report_of_every_run() {
        let file = String::from_utf8_lossy(&document(&measured(&[200.0, 250.0]), Duration::ZERO))
            .to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(250.0 Mbps)"));
        assert!(file.contains("(page 1 of 1)"));
    }
}
