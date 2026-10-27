//! Which subnet to sweep: the one typed on the command line, or the one this machine is on.

use crate::ping::target::gateway;
use anyhow::{Context, bail};
use ipnet::Ipv4Net;
use std::net::Ipv4Addr;

/// Sweeping more than this many addresses at the ping's pace would take minutes.
const WIDEST_PREFIX: u8 = 16;

pub struct Subnet {
    pub net: Ipv4Net,
    /// The interface it was found on; None when it was typed.
    pub interface: Option<String>,
    /// This machine's own address in the subnet, which answers pings but has no ARP entry.
    pub own: Option<Ipv4Addr>,
}

pub fn choose(typed: Option<&str>) -> anyhow::Result<Subnet> {
    let mine = interfaces();
    let mut subnet = match typed {
        Some(text) => parse(text)?,
        None => local(&mine, gateway()).context(
            "no network interface with an IPv4 address was found; give the subnet, like 192.168.1.0/24",
        )?,
    };
    subnet.own = mine
        .iter()
        .find(|(_, net)| subnet.net.contains(&net.addr()))
        .map(|(_, net)| net.addr());
    Ok(subnet)
}

/// `192.168.1.0/24`, or an address alone for its /24.
fn parse(text: &str) -> anyhow::Result<Subnet> {
    let net = if text.contains('/') {
        text.parse::<Ipv4Net>()
            .with_context(|| format!("{text} is not a subnet like 192.168.1.0/24"))?
    } else {
        let ip: Ipv4Addr = text
            .parse()
            .with_context(|| format!("{text} is not an address or a subnet like 192.168.1.0/24"))?;
        Ipv4Net::new(ip, 24)?
    };
    if net.prefix_len() < WIDEST_PREFIX {
        bail!("{net} is too big to sweep; a /{WIDEST_PREFIX} is the limit");
    }
    Ok(Subnet {
        net: net.trunc(),
        interface: None,
        own: None,
    })
}

/// Every connected interface's address with its subnet, loopback and link-local left out.
fn interfaces() -> Vec<(String, Ipv4Net)> {
    let Ok(found) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    found
        .into_iter()
        // An unplugged adapter can keep a fixed address it can't reach anything on.
        .filter(|interface| !interface.is_loopback() && interface.is_oper_up())
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(v4) if !v4.ip.is_link_local() => {
                let prefix = u32::from(v4.netmask).count_ones() as u8;
                Some((interface.name, Ipv4Net::new(v4.ip, prefix).ok()?))
            }
            _ => None,
        })
        .collect()
}

/// The LAN this machine is most likely on: a private range of a sweepable size, on a wired or
/// wireless interface (`en*`, `eth*`, `wl*`) before VPN tunnels and bridges, and the one the
/// gateway is on before the rest. Windows names its interfaces for people (`Wi-Fi`, `VirtualBox
/// Host-Only Network`), so there the gateway is what tells the LAN from a virtual machine's.
fn local(mine: &[(String, Ipv4Net)], gateway: Option<Ipv4Addr>) -> Option<Subnet> {
    let mut candidates: Vec<&(String, Ipv4Net)> = mine
        .iter()
        .filter(|(_, net)| (WIDEST_PREFIX..=30).contains(&net.prefix_len()))
        .collect();
    candidates.sort_by_key(|(name, net)| {
        let lan_like = name.starts_with("en") || name.starts_with("eth") || name.starts_with("wl");
        let has_gateway = gateway.is_some_and(|gateway| net.contains(&gateway));
        (
            !net.addr().is_private(),
            !lan_like,
            !has_gateway,
            name.clone(),
        )
    });
    let (name, net) = candidates.first()?;
    Some(Subnet {
        net: net.trunc(),
        interface: Some(name.clone()),
        own: Some(net.addr()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_subnet_or_an_address() {
        assert_eq!(
            parse("192.168.1.0/24").unwrap().net.to_string(),
            "192.168.1.0/24"
        );
        assert_eq!(
            parse("192.168.1.37/24").unwrap().net.to_string(),
            "192.168.1.0/24"
        );
        assert_eq!(
            parse("192.168.1.37").unwrap().net.to_string(),
            "192.168.1.0/24"
        );
        assert!(parse("10.0.0.0/8").is_err());
        assert!(parse("printer").is_err());
    }

    #[test]
    fn prefers_the_wired_or_wireless_lan() {
        let mine = vec![
            ("utun3".to_string(), "100.64.0.5/32".parse().unwrap()),
            ("bridge100".to_string(), "192.168.64.1/24".parse().unwrap()),
            ("en0".to_string(), "192.168.1.20/24".parse().unwrap()),
        ];
        let subnet = local(&mine, None).unwrap();
        assert_eq!(subnet.net.to_string(), "192.168.1.0/24");
        assert_eq!(subnet.interface.as_deref(), Some("en0"));
        assert_eq!(subnet.own, Some(Ipv4Addr::new(192, 168, 1, 20)));
    }

    #[test]
    fn prefers_the_gateways_lan_on_windows() {
        // VirtualBox's host-only network sorts before Wi-Fi by name.
        let mine = vec![
            (
                "VirtualBox Host-Only Network".to_string(),
                "192.168.56.1/24".parse().unwrap(),
            ),
            ("Wi-Fi".to_string(), "192.168.1.20/24".parse().unwrap()),
        ];
        let subnet = local(&mine, Some(Ipv4Addr::new(192, 168, 1, 1))).unwrap();
        assert_eq!(subnet.interface.as_deref(), Some("Wi-Fi"));
    }
}
