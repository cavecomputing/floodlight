//! The tables printed to stdout for `--plain` and `--find`: the same sweep, survey, speed test or
//! pings, no screen,
//! so it works in a pipe or a script.

use crate::ping::pings::{self, Ping, Pings};
use crate::ping::target::Target;
use crate::scan::hosts::{Host, Query, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep;
use crate::speedtest::runs::{self, Run};
use crate::ui::wireless;
use crate::wifi::channels::{self, Band, Crowd};
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
        "{:<24}  {:>3}  {:<7}  {:<7}  {:<7}  {:<18}  {:<30}  bssid",
        "ssid", "ch", "band", "width", "signal", "neighbours", "air"
    );
    for (network, crowd) in networks.iter().zip(channels::crowds(&networks)) {
        let [ssid, channel, band, width, signal, neighbours, bssid] = network_words(network, crowd);
        let air = channels::strip(network.freq_mhz, network.width_mhz, network.centre_mhz);
        println!(
            "{ssid:<24}  {channel:>3}  {band:<7}  {width:<7}  {signal:<7}  {neighbours:<18}  {air:<30}  {bssid}"
        );
    }
    Ok(())
}

/// A network's name, channel, band, width, signal, neighbours and BSSID as the printed tables
/// write them.
pub fn network_words(network: &Network, crowd: Crowd) -> [String; 7] {
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
        crowd.words(),
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
    let ms = |value: Option<f32>| value.map_or("—".to_string(), |ms| format!("{ms:.1} ms"));
    let mbps =
        |value: Option<f32>| value.map_or("—".to_string(), |mbps| format!("{mbps:.1} Mbps"));
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

/// Prints each ping as it comes back, then each host's totals once the last is in (with `count`
/// 0, it pings until stopped, like the OS's own ping). With several hosts each row says which.
pub fn ping(targets: &[Target], count: u32, wait: Duration) -> anyhow::Result<()> {
    let ips: Vec<Ipv4Addr> = targets.iter().map(|target| target.ip).collect();
    let events = pings::start(&ips, count, wait).map_err(anyhow::Error::msg)?;
    println!("# {}", ping_headline(targets, count, wait));
    // The host column, only when there is more than one.
    let labels = ping_labels(targets);
    let host = |label: &str| match targets.len() {
        1 => String::new(),
        _ => format!("{label:<width$}  ", width = labels[0].chars().count()),
    };
    println!("{:>5}  {:<8}  {}latency", "seq", "time", host("host"));
    let mut pings: Vec<Pings> = targets.iter().map(|_| Pings::default()).collect();
    for ping in events {
        let [seq, time, latency] = ping_words(&ping);
        println!("{seq:>5}  {time:<8}  {}{latency}", host(&labels[ping.host]));
        pings[ping.host].pings.push(ping);
    }
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
