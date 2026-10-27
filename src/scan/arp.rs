//! The OS's ARP table: which MAC answered for each address. Read through the system command,
//! which needs no privileges and exists on every OS.

use super::hosts::normalize_mac;
use regex::Regex;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::OnceLock;

/// One address's entry: its MAC, and whether the OS has heard from it lately. Only Linux's
/// `ip neigh` says: `STALE` (not since it last checked), `DELAY` and `PROBE` (checking now) may
/// be a host that has left, and the entry stays until a check fails. macOS and Windows don't
/// say, so theirs count as fresh.
pub struct Neighbour {
    pub mac: String,
    pub fresh: bool,
}

pub async fn read() -> HashMap<Ipv4Addr, Neighbour> {
    parse(&table_text().await)
}

/// The table as text. Linux: `ip neigh` (iproute2, always there), then `arp` (net-tools, often
/// not). macOS and Windows: `arp -a`.
async fn table_text() -> String {
    let commands: &[(&str, &[&str])] = if cfg!(target_os = "linux") {
        &[("ip", &["-4", "neigh", "show"]), ("arp", &["-an"])]
    } else if cfg!(windows) {
        &[("arp", &["-a"])]
    } else {
        &[("arp", &["-an"])]
    };
    for (program, args) in commands {
        if let Ok(output) = tokio::process::Command::new(program)
            .args(*args)
            .output()
            .await
            && output.status.success()
        {
            return String::from_utf8_lossy(&output.stdout).into_owned();
        }
    }
    String::new()
}

/// Every address, MAC and state, in any of the three formats:
/// macOS and Linux `arp -an`: `? (192.168.1.1) at b8:27:eb:12:34:56 on en0 ifscope [ethernet]`
/// Linux `ip neigh`:          `192.168.1.1 dev wlan0 lladdr b8:27:eb:12:34:56 REACHABLE`
/// Windows `arp -a`:          `  192.168.1.1          b8-27-eb-12-34-56     dynamic`
/// Entries without a MAC (`(incomplete)`, `FAILED`) don't match.
pub fn parse(text: &str) -> HashMap<Ipv4Addr, Neighbour> {
    static PAIR: OnceLock<Regex> = OnceLock::new();
    let pair = PAIR.get_or_init(|| {
        Regex::new(
            r"(?i)(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\)?\s+(?:at\s+|dev\s+\S+\s+lladdr\s+)?([0-9a-f]{1,2}(?:[:-][0-9a-f]{1,2}){5})\b(.*)",
        )
        .expect("the pattern is fixed")
    });
    let mut table = HashMap::new();
    for found in pair.captures_iter(text) {
        if let (Ok(ip), Some(mac)) = (found[1].parse(), normalize_mac(&found[2])) {
            let state = found[3].split_whitespace().last().unwrap_or_default();
            let fresh = !matches!(state, "STALE" | "DELAY" | "PROBE");
            table.insert(ip, Neighbour { mac, fresh });
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    #[test]
    fn reads_macos_and_linux_arp() {
        let text = "? (192.168.1.1) at b8:27:eb:12:34:56 on en0 ifscope [ethernet]\n\
                    ? (192.168.1.9) at e6:a:2b:3c:4d:5e on en0 ifscope [ethernet]\n\
                    ? (192.168.1.77) at (incomplete) on en0 ifscope [ethernet]\n\
                    ? (192.168.1.255) at ff:ff:ff:ff:ff:ff on en0 ifscope [ethernet]\n";
        let table = parse(text);
        assert_eq!(table.len(), 2);
        assert_eq!(table[&ip("192.168.1.1")].mac, "b8:27:eb:12:34:56");
        assert!(table[&ip("192.168.1.1")].fresh);
        assert_eq!(table[&ip("192.168.1.9")].mac, "e6:0a:2b:3c:4d:5e");
    }

    #[test]
    fn reads_ip_neigh() {
        let text = "192.168.1.1 dev wlan0 lladdr b8:27:eb:12:34:56 router REACHABLE\n\
                    192.168.1.40 dev wlan0  FAILED\n\
                    192.168.1.41 dev wlan0 lladdr 2c:a5:9c:0b:3d:57 STALE\n\
                    192.168.1.42 dev wlan0 lladdr 2c:a5:9c:0b:3d:58 DELAY\n";
        let table = parse(text);
        assert_eq!(table.len(), 3);
        assert!(table[&ip("192.168.1.1")].fresh);
        assert_eq!(table[&ip("192.168.1.41")].mac, "2c:a5:9c:0b:3d:57");
        assert!(!table[&ip("192.168.1.41")].fresh);
        assert!(!table[&ip("192.168.1.42")].fresh);
    }

    #[test]
    fn reads_windows_arp() {
        let text = "Interface: 192.168.1.20 --- 0x5\n\
                    \x20 Internet Address      Physical Address      Type\n\
                    \x20 192.168.1.1           b8-27-eb-12-34-56     dynamic\n\
                    \x20 192.168.1.255         ff-ff-ff-ff-ff-ff     static\n";
        let table = parse(text);
        assert_eq!(table.len(), 1);
        assert_eq!(table[&ip("192.168.1.1")].mac, "b8:27:eb:12:34:56");
        assert!(table[&ip("192.168.1.1")].fresh);
    }
}
