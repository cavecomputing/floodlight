//! The sweep as a report: the subnet, what answered, and a table of every device.

use super::sheet::{self, AQUA, COOL, Cell, Column, FG, MUTED, SUBTLE, Sheet};
use crate::scan::hosts::{Filter, Hosts, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep;
use crate::ui::plain;
use std::path::Path;
use tokio::runtime::Runtime;

const COLUMNS: [Column; 5] = [
    Column::mono("Address", 0.17),
    Column::mono("MAC", 0.19),
    Column::text("Maker", 0.25),
    Column::text("Name", 0.26),
    Column::number("Ping", 0.13),
];

pub fn write(runtime: &Runtime, subnet: Subnet, filter: &str, path: &Path) -> anyhow::Result<()> {
    let hosts = sweep::finished(runtime, subnet.net);
    std::fs::write(path, document(&hosts, &subnet, filter))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The report's PDF file.
pub fn document(hosts: &Hosts, subnet: &Subnet, filter: &str) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(&date, &time);

    let mut from = Vec::new();
    if let Some(interface) = &subnet.interface {
        from.push(format!("swept from {interface}"));
    }
    if let Some(own) = subnet.own {
        from.push(format!("this machine {own}"));
    }
    let mut facts = vec![
        ("date", date),
        ("time", time),
        (
            "answered",
            format!("{} of {}", hosts.rows.len(), hosts.total),
        ),
        (
            "took",
            format!("{:.1} s", hosts.took.unwrap_or_default().as_secs_f32()),
        ),
    ];
    if !filter.is_empty() {
        facts.push(("matching", filter.to_string()));
    }
    sheet.masthead(
        "Network sweep",
        &subnet.net.to_string(),
        &from.join(" · "),
        &facts,
    );

    let kept = Filter::new(filter);
    let rows: Vec<Vec<Cell>> = hosts
        .rows
        .values()
        .filter(|host| kept.keeps(host))
        .map(|host| {
            let [ip, mac, vendor, name, ping] = plain::host_words(host, subnet.own);
            // Anything the sweep couldn't learn is in the comment colour, like on screen.
            let ink = |known: bool, ink| if known { ink } else { SUBTLE };
            vec![
                (ip, COOL),
                (mac, ink(host.mac.is_some(), FG)),
                (vendor, ink(matches!(host.vendor, Vendor::Known(_)), MUTED)),
                (name, ink(host.name.is_some(), MUTED)),
                (ping, ink(host.latency_ms.is_some(), AQUA)),
            ]
        })
        .collect();
    sheet.heading("Devices", 36.0);
    if rows.is_empty() {
        sheet.note(if !kept.is_empty() {
            "Nothing matches."
        } else {
            "Nothing answered."
        });
    } else {
        sheet.table(&COLUMNS, &rows);
    }
    sheet.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::sweep::Event;
    use std::net::Ipv4Addr;
    use std::time::Duration;

    fn swept() -> (Hosts, Subnet) {
        let subnet = Subnet {
            net: "192.168.1.0/24".parse().expect("a subnet"),
            interface: Some("eth0".to_string()),
            own: Some(Ipv4Addr::new(192, 168, 1, 20)),
        };
        let mut hosts = Hosts::new(254);
        for last in 1..=80 {
            let ip = Ipv4Addr::new(192, 168, 1, last);
            hosts.apply(Event::Probed {
                ip,
                latency_ms: (last % 7 != 0).then_some(1.5),
            });
            let vendor = match last % 3 {
                0 => Vendor::Known("Example Networks".to_string()),
                1 => Vendor::Randomized,
                _ => Vendor::Unknown,
            };
            let mac = format!("02:00:00:00:3d:{last:02x}");
            hosts.apply(Event::Mac { ip, mac, vendor });
        }
        hosts.apply(Event::Name {
            ip: Ipv4Addr::new(192, 168, 1, 3),
            name: "doorcam.local".to_string(),
        });
        hosts.apply(Event::Done {
            took: Duration::from_secs(4),
        });
        (hosts, subnet)
    }

    #[test]
    fn writes_a_report_of_every_device() {
        let (hosts, subnet) = swept();
        let file = String::from_utf8_lossy(&document(&hosts, &subnet, "")).to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(192.168.1.0/24)"));
        assert!(file.contains("(doorcam.local)"));
        assert!(file.contains("(Page 3 of 3)"));
    }
}
