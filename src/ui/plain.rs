//! The tables printed to stdout for `--plain` and `--find`: the same sweep or survey, no screen,
//! so it works in a pipe or a script.

use crate::scan::hosts::{Hosts, Query, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep::{self, Event};
use crate::ui::wireless;
use crate::wifi::channels::{self, Band};
use crate::wifi::survey;
use tokio::runtime::Runtime;

pub fn run(runtime: &Runtime, subnet: Subnet, find: Option<&str>) -> anyhow::Result<()> {
    let mut events = sweep::start(runtime, subnet.net);
    let mut hosts = Hosts::new(subnet.net.hosts().count());
    runtime.block_on(async {
        while let Some(event) = events.recv().await {
            let done = matches!(event, Event::Done { .. });
            hosts.apply(event);
            if done {
                break;
            }
        }
    });

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
        println!("# nothing matches");
        return Ok(());
    }
    println!(
        "{:<15}  {:<17}  {:<30}  {:<30}  ping",
        "ip", "mac", "maker", "name"
    );
    for host in shown {
        let mac = host.mac.as_deref().unwrap_or("—");
        let vendor = match &host.vendor {
            Vendor::Known(name) => name.as_str(),
            Vendor::Randomized => "randomized",
            Vendor::Unknown => "—",
        };
        let name = match &host.name {
            Some(name) => name.as_str(),
            None if subnet.own == Some(host.ip) => "this machine",
            None => "—",
        };
        let ping = match host.latency_ms {
            Some(ms) => format!("{ms:.1} ms"),
            None => "no reply".to_string(),
        };
        println!(
            "{:<15}  {:<17}  {:<30}  {:<30}  {}",
            host.ip, mac, vendor, name, ping
        );
    }
    Ok(())
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
        let mut ssid = network.ssid.clone().unwrap_or_else(|| "hidden".to_string());
        if network.connected {
            ssid.push_str(" · joined");
        }
        println!(
            "{:<24}  {:>3}  {:<7}  {:<7}  {:<7}  {:<18}  {:<30}  {}",
            ssid,
            channels::channel(network.freq_mhz),
            Band::of(network.freq_mhz).name(),
            wireless::width(network),
            format!("{} dBm", network.signal_dbm),
            crowd.words(),
            channels::strip(network.freq_mhz, network.width_mhz),
            network.bssid.as_deref().unwrap_or("—"),
        );
    }
    Ok(())
}
