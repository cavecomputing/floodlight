//! The speed test as a report: download and upload up top, what they mean in a few sentences,
//! a chart of each result across the runs when there are several, and a table of every run.

use super::pdf::{self, Font};
use super::sheet::{
    self, ACCENT, AQUA, BORDER, COOL, Cell, Column, FG, LEFT, MUTED, SUBTLE, Sheet, WIDTH,
};
use crate::speedtest::runs::{self, Run, Runs};
use crate::ui::plain;
use std::path::Path;
use std::time::Duration;

const COLUMNS: [Column; 6] = [
    Column::mono("Time", 0.12),
    Column::number("Latency", 0.12),
    Column::number("Jitter", 0.11),
    Column::number("Download", 0.15),
    Column::number("Upload", 0.15),
    Column::text("Server", 0.35),
];

/// Runs the test `count` times, `wait` apart, saying how each went, then writes the report.
pub fn write(path: &Path, count: u32, wait: Duration) -> anyhow::Result<()> {
    let mut ended = 0;
    let runs = runs::finished(count, wait, |run| {
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
    let mut sheet = Sheet::new(&date, &time);

    let title = format!(
        "{} down · {} up",
        plain::mbps(runs.median(Run::download)),
        plain::mbps(runs.median(Run::upload))
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
        ("latency", plain::ms(runs.median(Run::latency))),
        ("jitter", plain::ms(runs.median(Run::jitter))),
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
    if runs.runs.len() > 1 {
        let times: Vec<&str> = runs.runs.iter().map(|run| run.time.as_str()).collect();
        let each = |result: fn(&Run) -> Option<f32>| runs.runs.iter().map(result).collect();
        chart(
            &mut sheet,
            "Download across runs",
            &times,
            each(Run::download),
            "Mbps",
            AQUA,
        );
        chart(
            &mut sheet,
            "Upload across runs",
            &times,
            each(Run::upload),
            "Mbps",
            COOL,
        );
        chart(
            &mut sheet,
            "Latency across runs",
            &times,
            each(Run::latency),
            "ms",
            ACCENT,
        );
    }
    sheet.heading("Runs", 36.0);
    sheet.table(&COLUMNS, &rows);
    sheet.finish()
}

const CHART_HEIGHT: f32 = 90.0;
/// The chart's left column, where the scale's numbers go.
const SCALE: f32 = 64.0;

/// One result across the runs as a line in a card: a point a run, joined to the next, with the
/// scale on the left and the runs' times along the bottom. A run that didn't measure it leaves a
/// gap. The ping report draws its chart the same way.
pub(super) fn chart(
    sheet: &mut Sheet,
    title: &str,
    times: &[&str],
    values: Vec<Option<f32>>,
    unit: &str,
    ink: pdf::Rgb,
) {
    sheet.heading(title, CHART_HEIGHT);
    let (x, y) = (LEFT, sheet.y);
    let pdf = &mut sheet.pdf;
    let (top, bottom) = (y + 16.0, y + CHART_HEIGHT - 26.0);
    let (left, right) = (x + SCALE, x + WIDTH - 24.0);
    let most = values.iter().flatten().copied().fold(0.0, f32::max);
    let ceiling = runs::scale_top(most);
    for share in [0.0, 0.5, 1.0] {
        let line = bottom - share * (bottom - top);
        pdf.line((left, line), (right, line), 0.5, BORDER);
        let label = format!("{} {unit}", ceiling * share);
        let label_left = left - 8.0 - pdf::width(Font::Mono, 6.0, &label);
        pdf.text(label_left, line + 2.0, Font::Mono, 6.0, SUBTLE, &label);
    }

    let step = (right - left) / (times.len() - 1).max(1) as f32;
    let at = |i: usize, value: f32| {
        (
            left + step * i as f32,
            bottom - value / ceiling * (bottom - top),
        )
    };
    // Every time would crowd the axis past a dozen runs; every few is enough to read it by.
    let every = times.len().div_ceil(12);
    for (i, time) in times.iter().enumerate().step_by(every) {
        let centre = left + step * i as f32;
        let label_left = centre - pdf::width(Font::Mono, 6.0, time) / 2.0;
        pdf.text(label_left, bottom + 14.0, Font::Mono, 6.0, SUBTLE, time);
    }
    // A light wash of the line's colour under each unbroken run of it.
    let wash = faded(ink);
    let mut run: Vec<(f32, f32)> = Vec::new();
    for (i, value) in values.iter().chain([&None]).enumerate() {
        match value {
            Some(value) => run.push(at(i, *value)),
            None => {
                if run.len() > 1 {
                    let (first, last) = (run[0].0, run[run.len() - 1].0);
                    run.extend([(last, bottom), (first, bottom)]);
                    pdf.polygon(&run, wash);
                }
                run.clear();
            }
        }
    }
    for (i, pair) in values.windows(2).enumerate() {
        if let [Some(from), Some(to)] = pair {
            pdf.line(at(i, *from), at(i + 1, *to), 1.1, ink);
        }
    }
    for (i, value) in values.iter().enumerate() {
        if let Some(value) = value {
            let (cx, cy) = at(i, *value);
            pdf.bar(cx - 1.5, cy - 1.5, 3.0, 3.0, ink);
        }
    }
    sheet.y += CHART_HEIGHT + 4.0;
}

/// A colour faded most of the way to white, for the wash under a chart's line.
fn faded((r, g, b): pdf::Rgb) -> pdf::Rgb {
    let fade = |part: u8| (part as f32 * 0.12 + 255.0 * 0.88) as u8;
    (fade(r), fade(g), fade(b))
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

/// `210.4 to 250.3 Mbps`: the slowest and fastest of one result across the runs. The LAN speed
/// report says its ranges the same way.
pub(super) fn range<R>(runs: &[&R], result: fn(&R) -> Option<f32>) -> String {
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
        assert!(file.contains("(Page 1 of 1)"));
    }
}
