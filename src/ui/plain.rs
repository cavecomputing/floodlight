//! The table printed to stdout for `--plain` and `--find`: the same sweep, no screen, so it
//! works in a pipe or a script.

use crate::scan::hosts::{Hosts, Query, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep::{self, Event};
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
