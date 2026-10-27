//! What to ping: the name or address typed, or this machine's gateway when nothing was.

use anyhow::{Context, bail};
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

pub struct Target {
    pub ip: Ipv4Addr,
    /// `gateway`, or the name typed when it wasn't an address.
    pub name: Option<String>,
}

impl Target {
    /// `gateway (192.168.1.1)`, `example.com (192.0.2.7)`, or the address alone.
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} ({})", self.ip),
            None => self.ip.to_string(),
        }
    }
}

pub fn choose(typed: Option<&str>) -> anyhow::Result<Target> {
    let Some(text) = typed else {
        let ip = gateway()
            .context("no gateway was found; give the host to ping, like --ping 1.1.1.1")?;
        return Ok(Target {
            ip,
            name: Some("gateway".to_string()),
        });
    };
    if let Ok(ip) = text.parse() {
        return Ok(Target { ip, name: None });
    }
    let found = (text, 0)
        .to_socket_addrs()
        .with_context(|| format!("{text} is not an address, and no name server knows it"))?;
    for address in found {
        if let IpAddr::V4(ip) = address.ip() {
            return Ok(Target {
                ip,
                name: Some(text.to_string()),
            });
        }
    }
    bail!("{text} has only IPv6 addresses, and pings go over IPv4 for now")
}

/// The router this machine sends the internet's traffic through, from the OS's route table.
#[cfg(windows)]
pub fn gateway() -> Option<Ipv4Addr> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetBestRoute, MIB_IPFORWARDROW};
    // SAFETY: a zeroed route is a valid one to fill in: numbers only.
    let mut route: MIB_IPFORWARDROW = unsafe { std::mem::zeroed() };
    // The route Windows would take to an address out on the internet; its next hop is the
    // gateway. SAFETY: the route is a live local of the size the call expects.
    let found = unsafe { GetBestRoute(u32::from_ne_bytes([1, 1, 1, 1]), 0, &mut route) };
    let ip = Ipv4Addr::from(route.dwForwardNextHop.to_ne_bytes());
    (found == 0 && !ip.is_unspecified()).then_some(ip)
}

/// Linux reads its route table from /proc; macOS asks `route`.
#[cfg(not(windows))]
pub fn gateway() -> Option<Ipv4Addr> {
    if cfg!(target_os = "macos") {
        let output = std::process::Command::new("route")
            .args(["-n", "get", "default"])
            .output()
            .ok()?;
        macos_gateway(&String::from_utf8_lossy(&output.stdout))
    } else {
        linux_gateway(&std::fs::read_to_string("/proc/net/route").ok()?)
    }
}

/// The default route's gateway in /proc/net/route: destination `00000000`, the gateway flag (2)
/// set, the address in hex with its bytes the wrong way round.
#[cfg_attr(windows, allow(dead_code))]
fn linux_gateway(table: &str) -> Option<Ipv4Addr> {
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let flags = u16::from_str_radix(fields.get(3)?, 16).ok()?;
        if *fields.get(1)? != "00000000" || flags & 2 == 0 {
            return None;
        }
        let hex = u32::from_str_radix(fields.get(2)?, 16).ok()?;
        Some(Ipv4Addr::from(hex.to_le_bytes()))
    })
}

/// The `gateway: 192.168.1.1` line of `route -n get default`.
#[cfg_attr(windows, allow(dead_code))]
fn macos_gateway(output: &str) -> Option<Ipv4Addr> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("gateway:"))?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_gateway_from_linux_routes() {
        let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                     wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n\
                     wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n";
        assert_eq!(linux_gateway(table), Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(
            linux_gateway(
                table
                    .lines()
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .as_str()
            ),
            None
        );
    }

    #[test]
    fn reads_the_gateway_from_macos_route() {
        let output = "   route to: default\ndestination: default\n       mask: default\n    \
                      gateway: 192.168.1.1\n  interface: en0\n";
        assert_eq!(macos_gateway(output), Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(
            macos_gateway("route: writing to routing socket: not in table\n"),
            None
        );
    }

    #[test]
    fn takes_an_address_as_it_is() {
        let target = choose(Some("192.168.1.20")).unwrap();
        assert_eq!(target.ip, Ipv4Addr::new(192, 168, 1, 20));
        assert_eq!(target.label(), "192.168.1.20");
    }
}
