//! The sweep as a report: the subnet, what answered, and a table of every device.

use super::sheet::{self, AQUA, COOL, Cell, Column, FG, MUTED, SUBTLE, Sheet};
use crate::scan::hosts::{Hosts, Query, Vendor};
use crate::scan::subnet::Subnet;
use crate::scan::sweep;
use crate::ui::plain;
use std::path::Path;
use tokio::runtime::Runtime;

const COLUMNS: [Column; 5] = [
    Column {
        label: "address",
        width: 0.17,
        mono: true,
        right: false,
    },
    Column {
        label: "mac",
        width: 0.19,
        mono: true,
        right: false,
    },
    Column {
        label: "maker",
        width: 0.25,
        mono: false,
        right: false,
    },
    Column {
        label: "name",
        width: 0.26,
        mono: false,
        right: false,
    },
    Column {
        label: "ping",
        width: 0.13,
        mono: false,
        right: true,
    },
];

pub fn write(
    runtime: &Runtime,
    subnet: Subnet,
    find: Option<&str>,
    path: &Path,
) -> anyhow::Result<()> {
    let hosts = sweep::finished(runtime, subnet.net);
    std::fs::write(path, document(&hosts, &subnet, find))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The report's PDF file.
pub fn document(hosts: &Hosts, subnet: &Subnet, find: Option<&str>) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(format!("Network sweep · {} · {date} {time}", subnet.net));

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
    if let Some(find) = find {
        facts.push(("matching", find.to_string()));
    }
    sheet.masthead(
        "Network sweep",
        &subnet.net.to_string(),
        &from.join(" · "),
        &facts,
    );
    sheet.summary(&summary(hosts, subnet, find));

    let query = Query::new(find.unwrap_or(""));
    let rows: Vec<Vec<Cell>> = hosts
        .rows
        .values()
        .filter(|host| host.shows(&query))
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
        sheet.summary(if find.is_some() {
            "Nothing matches."
        } else {
            "Nothing answered."
        });
    } else {
        sheet.table(&COLUMNS, &rows);
    }
    sheet.finish()
}

/// What answered, and what the makers say, in a sentence or three.
fn summary(hosts: &Hosts, subnet: &Subnet, find: Option<&str>) -> String {
    let count = |want: fn(&Vendor) -> bool| hosts.rows.values().filter(|h| want(&h.vendor)).count();
    let known = count(|vendor| matches!(vendor, Vendor::Known(_)));
    let randomized = count(|vendor| *vendor == Vendor::Randomized);
    // Only a row with a MAC has a maker to look up; one found by ping alone has none.
    let unknown = hosts
        .rows
        .values()
        .filter(|h| h.mac.is_some() && h.vendor == Vendor::Unknown)
        .count();
    let silent = hosts
        .rows
        .values()
        .filter(|h| h.latency_ms.is_none())
        .count();
    let mut text = format!(
        "{} of the {} addresses on {} answered.",
        hosts.rows.len(),
        hosts.total,
        subnet.net
    );
    let mut makers = Vec::new();
    if known > 0 {
        makers.push(format!("{known} are named by their maker"));
    }
    if randomized > 0 {
        makers.push(format!(
            "{randomized} use a randomized MAC (usually a phone or laptop keeping its real one private)"
        ));
    }
    if unknown > 0 {
        makers.push(format!("{unknown} have a maker the registry doesn't list"));
    }
    match makers.as_slice() {
        [] => {}
        [only] => text.push_str(&format!(" Of those, {only}.")),
        [first @ .., last] => {
            text.push_str(&format!(" Of those, {}, and {last}.", first.join(", ")))
        }
    }
    if silent > 0 {
        text.push_str(&format!(
            " {silent} ignored the ping but turned up in this machine's address table."
        ));
    }
    if let Some(find) = find {
        let query = Query::new(find);
        let shown = hosts.rows.values().filter(|h| h.shows(&query)).count();
        text.push_str(&format!(
            " The table lists the {shown} that match \"{find}\"."
        ));
    }
    text
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
    fn sums_up_what_answered() {
        let (hosts, subnet) = swept();
        assert_eq!(
            summary(&hosts, &subnet, Some("doorcam")),
            "80 of the 254 addresses on 192.168.1.0/24 answered. Of those, 26 are named by their \
             maker, 27 use a randomized MAC (usually a phone or laptop keeping its real one \
             private), and 27 have a maker the registry doesn't list. 11 ignored the ping but \
             turned up in this machine's address table. The table lists the 1 that match \
             \"doorcam\"."
        );
    }

    #[test]
    fn writes_a_report_of_every_device() {
        let (hosts, subnet) = swept();
        let file = String::from_utf8_lossy(&document(&hosts, &subnet, None)).to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(192.168.1.0/24)"));
        assert!(file.contains("(doorcam.local)"));
        assert!(file.contains("(page 3 of 3)"));
    }
}
