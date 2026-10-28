//! The tables printed to stdout for `--plain` and `--find`: the same sweep, survey, speed test,
//! pings or trace, no screen, so it works in a pipe or a script.

use crate::lanspeed::runs::{self as lanspeed_runs, Event as LanEvent, Settings};
use crate::lanspeed::wire::{Direction, Report};
use crate::ping::pings::{self, Ping, Pings};
use crate::ping::target::Target;
use crate::ping::trace::{self, Hop, Trace};
use crate::scan::hosts::{Host, Query, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep;
use crate::speedtest::runs::{self, Run};
use crate::ui::wireless;
use crate::wifi::channels::{self, Band};
use crate::wifi::survey::{self, Network};
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::runtime::Runtime;

pub fn run(runtime: &Runtime, subnet: Subnet, find: Option<&str>) -> anyhow::Result<()> {
    let hosts = sweep::finished(runtime, subnet.net);
    let query = Query::new(find.unwrap_or(""));
    let shown: Vec<_> = hosts
        .rows
        .values()
        .filter(|host| host.shows(&query))
        .collect();
    println!(
        "# {} · {} up · {:.1}s",
        subnet.net,
        hosts.rows.len(),
        hosts.took.unwrap_or_default().as_secs_f32()
    );
    if shown.is_empty() {
        let why = if query.is_empty() {
            "nothing answered"
        } else {
            "nothing matches"
        };
        println!("# {why}");
        return Ok(());
    }
    println!(
        "{:<15}  {:<17}  {:<30}  {:<30}  ping",
        "ip", "mac", "maker", "name"
    );
    for host in shown {
        let [ip, mac, vendor, name, ping] = host_words(host, subnet.own);
        println!("{ip:<15}  {mac:<17}  {vendor:<30}  {name:<30}  {ping}");
    }
    Ok(())
}

/// A host's address, MAC, maker, name and ping as the printed tables write them.
pub fn host_words(host: &Host, own: Option<Ipv4Addr>) -> [String; 5] {
    let vendor = match &host.vendor {
        Vendor::Known(name) => name.as_str(),
        Vendor::Randomized => "randomized",
        Vendor::Unknown => "—",
    };
    let name = match &host.name {
        Some(name) => name.as_str(),
        None if own == Some(host.ip) => "this machine",
        None => "—",
    };
    let ping = match host.latency_ms {
        Some(ms) => format!("{ms:.1} ms"),
        None => "no reply".to_string(),
    };
    [
        host.ip.to_string(),
        host.mac.clone().unwrap_or_else(|| "—".to_string()),
        vendor.to_string(),
        name.to_string(),
        ping,
    ]
}

pub fn wireless(runtime: &Runtime) -> anyhow::Result<()> {
    let networks = runtime
        .block_on(survey::survey())
        .map_err(anyhow::Error::msg)?;
    println!("# {}", wireless::headline(&networks));
    if networks.is_empty() {
        println!("# nothing heard");
        return Ok(());
    }
    println!(
        "{:<24}  {:>3}  {:<7}  {:<7}  {:<7}  {:<30}  bssid",
        "ssid", "ch", "band", "width", "signal", "air"
    );
    for network in &networks {
        let [ssid, channel, band, width, signal, bssid] = network_words(network);
        let air = channels::strip(network.freq_mhz, network.width_mhz, network.centre_mhz);
        println!(
            "{ssid:<24}  {channel:>3}  {band:<7}  {width:<7}  {signal:<7}  {air:<30}  {bssid}"
        );
    }
    Ok(())
}

/// A network's name, channel, band, width, signal and BSSID as the printed tables write them.
pub fn network_words(network: &Network) -> [String; 6] {
    let mut ssid = network.ssid.clone().unwrap_or_else(|| "hidden".to_string());
    if network.connected {
        ssid.push_str(" · joined");
    }
    [
        ssid,
        channels::channel(network.freq_mhz).to_string(),
        Band::of(network.freq_mhz).name().to_string(),
        wireless::width(network),
        format!("{} dBm", network.signal_dbm),
        network.bssid.clone().unwrap_or_else(|| "—".to_string()),
    ]
}

pub fn speedtest(count: u32, wait: Duration) -> anyhow::Result<()> {
    println!(
        "# speed test against cloudflare · {count} run{}",
        if count == 1 { "" } else { "s" }
    );
    println!(
        "{:<8}  {:>8}  {:>8}  {:>12}  {:>12}  server",
        "time", "latency", "jitter", "download", "upload"
    );
    runs::finished(count, wait, |run| {
        let [time, latency, jitter, download, upload, server] = run_words(run);
        println!("{time:<8}  {latency:>8}  {jitter:>8}  {download:>12}  {upload:>12}  {server}");
    });
    Ok(())
}

/// A run's time, latency, jitter, download, upload and server (or why it failed) as the printed
/// tables write them.
pub fn run_words(run: &Run) -> [String; 6] {
    let server = match (&run.failed, &run.server) {
        (Some(why), _) => format!("failed: {why}"),
        (None, Some(server)) => server.place(),
        (None, None) => "—".to_string(),
    };
    [
        run.time.clone(),
        ms(run.latency()),
        ms(run.jitter()),
        mbps(run.download()),
        mbps(run.upload()),
        server,
    ]
}

/// Prints a row an interval as the data arrives, and each transfer's total as a `#` line.
pub fn lanspeed(settings: Settings, count: u32, wait: Duration) -> anyhow::Result<()> {
    println!("# {}", lanspeed_headline(&settings, count));
    let udp = settings.protocol == crate::lanspeed::wire::Protocol::Udp;
    let titles = ["interval", "way", "bitrate", "transfer", "jitter", "lost"];
    let shown = if udp { 6 } else { 4 };
    println!("{}", lanspeed_line(&titles[..shown]));
    let runs = lanspeed_runs::finished(settings, count, wait, |event| match event {
        LanEvent::Interval(direction, report) => {
            let words = lanspeed_row(*direction, report, false);
            println!("{}", lanspeed_line(&words));
        }
        LanEvent::Total(direction, report) => {
            println!("# {}", lanspeed_row(*direction, report, true).join(" · "));
        }
        LanEvent::Failed(why) => println!("# failed: {why}"),
        _ => {}
    });
    if runs.runs.len() > 1 {
        println!("# {}", lanspeed_medians(&runs).unwrap_or_default());
    }
    Ok(())
}

/// A row's words in the printed table's columns.
fn lanspeed_line<S: AsRef<str>>(words: &[S]) -> String {
    let word = |i: usize| words.get(i).map_or("", AsRef::as_ref);
    let mut line = format!(
        "{:<11}  {:<9}  {:>12}  {:>10}",
        word(0),
        word(1),
        word(2),
        word(3)
    );
    if words.len() > 4 {
        line.push_str(&format!("  {:>8}  {}", word(4), word(5)));
    }
    line
}

/// `lanspeed to 192.168.1.20 · tcp · 10 s · 1 stream`, with the rate for UDP and the runs when
/// there are several.
pub fn lanspeed_headline(settings: &Settings, count: u32) -> String {
    let mut words = format!(
        "lanspeed to {} · {} · {} s · {} stream{}",
        settings.host,
        settings.protocol.name(),
        settings.seconds.as_secs_f32(),
        settings.streams,
        if settings.streams == 1 { "" } else { "s" }
    );
    if settings.protocol == crate::lanspeed::wire::Protocol::Udp {
        match settings.mbps {
            0.0 => words.push_str(" · as fast as it can"),
            mbps => words.push_str(&format!(" · {mbps} Mbps")),
        }
    }
    if let [direction] = settings.directions.as_slice() {
        words.push_str(&format!(" · {} only", direction.name()));
    }
    if count > 1 {
        words.push_str(&format!(" · {count} runs"));
    }
    words
}

/// `median 941.2 Mbps up · 938.0 Mbps down`, once more than one run measured.
pub fn lanspeed_medians(runs: &lanspeed_runs::Runs) -> Option<String> {
    use crate::lanspeed::runs::Run;
    let up = runs.median(Run::upload_mbps);
    let down = runs.median(Run::download_mbps);
    if up.is_none() && down.is_none() {
        return None;
    }
    let mut words = Vec::new();
    if let Some(up) = up {
        words.push(format!("{up:.1} Mbps up"));
    }
    if let Some(down) = down {
        words.push(format!("{down:.1} Mbps down"));
    }
    Some(format!("median {}", words.join(" · ")))
}

/// An interval's (or a total's) span, direction, bitrate and bytes moved, then for UDP the
/// jitter and the datagrams lost, as the printed tables write them.
pub fn lanspeed_row(direction: Direction, report: &Report, total: bool) -> Vec<String> {
    let span = match total {
        true => format!("{:.1} s total", report.to),
        false => format!("{:.1}–{:.1} s", report.from, report.to),
    };
    let mut words = vec![
        span,
        direction.name().to_string(),
        format!("{:.1} Mbps", report.mbps()),
        bytes_words(report.bytes),
    ];
    if let Some(datagrams) = report.datagrams {
        words.push(format!("{:.2} ms", datagrams.jitter_ms));
        words.push(format!(
            "{} ({:.1}%)",
            datagrams.lost,
            report.loss().unwrap_or_default()
        ));
    }
    words
}

/// `941.2 Mbps`, or `—` for a speed not measured.
pub fn mbps(value: Option<f32>) -> String {
    value.map_or("—".to_string(), |mbps| format!("{mbps:.1} Mbps"))
}

/// `12.3 ms`, or `—` for a time not measured.
pub fn ms(value: Option<f32>) -> String {
    value.map_or("—".to_string(), |ms| format!("{ms:.1} ms"))
}

/// `1.2 GB`, `117.0 MB`, `350 kB`: the bytes moved.
pub fn bytes_words(bytes: u64) -> String {
    match bytes {
        0..1_000_000 => format!("{:.0} kB", bytes as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", bytes as f64 / 1e6),
        _ => format!("{:.2} GB", bytes as f64 / 1e9),
    }
}

/// Prints each ping as it comes back, then each host's totals once the last is in (with `count`
/// 0, it pings until stopped, like the OS's own ping). With several hosts each row says which.
pub fn ping(targets: &[Target], count: u32, wait: Duration) -> anyhow::Result<()> {
    let ips: Vec<Ipv4Addr> = targets.iter().map(|target| target.ip).collect();
    println!("# {}", ping_headline(targets, count, wait));
    // The host column, only when there is more than one.
    let labels = ping_labels(targets);
    let host = |label: &str| match targets.len() {
        1 => String::new(),
        _ => format!("{label:<width$}  ", width = labels[0].chars().count()),
    };
    println!("{:>5}  {:<8}  {}latency", "seq", "time", host("host"));
    let pings = pings::finished(&ips, count, wait, |ping| {
        let [seq, time, latency] = ping_words(ping);
        println!("{seq:>5}  {time:<8}  {}{latency}", host(&labels[ping.host]));
    })
    .map_err(anyhow::Error::msg)?;
    for line in ping_totals_lines(targets, &pings) {
        println!("# {line}");
    }
    Ok(())
}

/// `ping gateway (192.168.1.1) · 10 pings · 500 ms apart`, or `ping 3 hosts · ...`
pub fn ping_headline(targets: &[Target], count: u32, wait: Duration) -> String {
    let pings = match count {
        0 => "until stopped".to_string(),
        1 => "1 ping".to_string(),
        count => format!("{count} pings"),
    };
    let hosts = match targets {
        [target] => target.label(),
        _ => format!("{} hosts", targets.len()),
    };
    format!("ping {hosts} · {pings} · {} ms apart", wait.as_millis())
}

/// Each host's label, padded to the longest so the columns after it line up.
pub fn ping_labels(targets: &[Target]) -> Vec<String> {
    let labels: Vec<String> = targets.iter().map(Target::label).collect();
    let width = labels.iter().map(|label| label.chars().count()).max();
    labels
        .iter()
        .map(|label| format!("{label:<width$}", width = width.unwrap_or_default()))
        .collect()
}

/// Each host's totals, led by its label when there are several.
pub fn ping_totals_lines(targets: &[Target], pings: &[Pings]) -> Vec<String> {
    let labels = ping_labels(targets);
    labels
        .iter()
        .zip(pings)
        .map(|(label, pings)| match targets.len() {
            1 => ping_totals(pings),
            _ => format!("{label} · {}", ping_totals(pings)),
        })
        .collect()
}

/// A ping's number, time and round trip (or why nothing came back) as the printed tables write
/// them.
pub fn ping_words(ping: &Ping) -> [String; 3] {
    let latency = match &ping.reply {
        Ok(ms) => format!("{ms:.2} ms"),
        Err(why) => why.clone(),
    };
    [ping.seq.to_string(), ping.time.clone(), latency]
}

/// `10 sent · 1 lost (10.0%) · min 0.41 · avg 1.10 · max 3.20 · jitter 0.30 ms`
pub fn ping_totals(pings: &Pings) -> String {
    let lost = pings.lost();
    let mut words = format!(
        "{} sent · {lost} lost ({:.1}%)",
        pings.pings.len(),
        pings.loss()
    );
    if let (Some(min), Some(average), Some(max)) = (pings.min(), pings.average(), pings.max()) {
        words.push_str(&format!(
            " · min {min:.2} · avg {average:.2} · max {max:.2}"
        ));
        match pings.jitter() {
            Some(jitter) => words.push_str(&format!(" · jitter {jitter:.2} ms")),
            None => words.push_str(" ms"),
        }
    }
    words
}

/// Traces `count` rounds, then prints a row a hop on the way.
pub fn trace(target: &Target, count: u32, wait: Duration) -> anyhow::Result<()> {
    println!("# {}", trace_headline(target, count, wait));
    let trace = trace::finished(target.ip, count, wait).map_err(anyhow::Error::msg)?;
    println!("# {}", trace_verdict(&trace));
    let titles = TRACE_TITLES.map(String::from);
    println!("{}", trace_line(&titles));
    for (at, hop) in trace.path().iter().enumerate() {
        println!("{}", trace_line(&trace_words(at, hop)));
    }
    Ok(())
}

pub const TRACE_TITLES: [&str; 8] = [
    "hop", "address", "name", "loss", "last", "min", "avg", "max",
];

fn trace_line(words: &[String; 8]) -> String {
    let [hop, address, name, loss, last, min, average, max] = words;
    format!(
        "{hop:>3}  {address:<19}  {name:<36}  {loss:>6}  {last:>9}  {min:>9}  {average:>9}  {max:>9}"
    )
}

/// `trace example.com (192.0.2.7) · 10 rounds · 1000 ms apart`
pub fn trace_headline(target: &Target, count: u32, wait: Duration) -> String {
    let rounds = match count {
        0 => "until stopped".to_string(),
        1 => "1 round".to_string(),
        count => format!("{count} rounds"),
    };
    format!(
        "trace {} · {rounds} · {} ms apart",
        target.label(),
        wait.as_millis()
    )
}

/// What the trace has found so far: how far it got, and where the loss starts.
pub fn trace_verdict(trace: &Trace) -> String {
    let hops = trace.path().len();
    if !trace.reached() {
        return match hops {
            0 => "nothing has answered yet".to_string(),
            _ => format!("{} hasn't answered · last heard at hop {hops}", trace.ip),
        };
    }
    let plural = if hops == 1 { "" } else { "s" };
    match (trace.loss_starts(), trace.path().last()) {
        (Some(at), Some(host)) => format!(
            "reached in {hops} hop{plural} · loss starts at hop {at} · {:.1}% lost at the host",
            host.pings.loss()
        ),
        _ => format!("reached in {hops} hop{plural} · no loss reaches the host"),
    }
}

/// A hop's number, router (`+1` for each other one heard there), its name, the share lost, and
/// its last, quickest, average and slowest round trips, as the printed tables write them.
pub fn trace_words(at: usize, hop: &Hop) -> [String; 8] {
    let (address, name) = match hop.routers.as_slice() {
        // Its first echo not back yet, while the host already answered a further one.
        [] if hop.pings.pings.is_empty() => ("waiting".to_string(), String::new()),
        [] => ("no reply".to_string(), String::new()),
        [(router, name), others @ ..] => {
            let more = match others.len() {
                0 => String::new(),
                more => format!(" +{more}"),
            };
            let name = name.clone().unwrap_or_else(|| "—".to_string());
            (format!("{router}{more}"), name)
        }
    };
    let last = match hop.pings.pings.last().map(|ping| &ping.reply) {
        Some(Ok(took)) => ms(Some(*took)),
        Some(Err(why)) => why.clone(),
        None => "—".to_string(),
    };
    [
        (at + 1).to_string(),
        address,
        name,
        format!("{:.1}%", hop.pings.loss()),
        last,
        ms(hop.pings.min()),
        ms(hop.pings.average()),
        ms(hop.pings.max()),
    ]
}
