//! The subnet calculator: an address and its prefix (or mask) worked out into the network, the
//! broadcast, the usable range, the mask and the wildcard. IPv4 and IPv6, no network, no UI.

use anyhow::{Context, bail};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// An address typed with its prefix length, like 192.168.1.50/24.
pub struct Subnet {
    pub address: IpAddr,
    pub prefix: u32,
}

/// Reads `10.0.4.0/22`, `192.168.1.0/255.255.252.0`, `192.168.1.0 255.255.252.0` or
/// `2001:db8::/48`.
pub fn parse(typed: &str) -> anyhow::Result<Subnet> {
    let Some((address, suffix)) = typed.trim().split_once(['/', ' ']) else {
        bail!("{typed} needs a prefix or a mask, like 192.168.1.0/24 or 192.168.1.0 255.255.255.0");
    };
    let address: IpAddr = address
        .parse()
        .with_context(|| format!("{address} isn't an IP address"))?;
    let suffix = suffix.trim();
    let bits = width(address);
    let prefix = match (suffix.parse::<u32>(), suffix.parse::<Ipv4Addr>()) {
        (Ok(prefix), _) if prefix <= bits => prefix,
        (Ok(_), _) => bail!("the prefix is 0 to {bits} for {address}"),
        (_, Ok(mask)) if address.is_ipv4() => {
            let mask = u32::from(mask);
            if mask.leading_ones() + mask.trailing_zeros() != 32 {
                bail!("{suffix} isn't a mask: its ones must all come first, like 255.255.252.0");
            }
            mask.leading_ones()
        }
        _ => bail!("{suffix} isn't a prefix length (like 24) or a mask (like 255.255.255.0)"),
    };
    Ok(Subnet { address, prefix })
}

impl Subnet {
    /// The label and value of each line printed, in order: the same for the screen and a test.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let host_bits = width(self.address) - self.prefix;
        let hosts = match host_bits {
            128 => u128::MAX,
            bits => (1 << bits) - 1,
        };
        let all = match self.address {
            IpAddr::V4(_) => u128::from(u32::MAX),
            IpAddr::V6(_) => u128::MAX,
        };
        let network = number(self.address) & !hosts;
        let last = network | hosts;
        let ip = |value: u128| match self.address {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::from(value as u32)),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::from(value)),
        };
        let mut rows = vec![
            ("address", self.address.to_string()),
            ("network", format!("{}/{}", ip(network), self.prefix)),
        ];
        if self.address.is_ipv6() {
            // IPv6 has no broadcast or masks written out; every address in the prefix is usable.
            rows.extend([
                ("first", ip(network).to_string()),
                ("last", ip(last).to_string()),
                ("addresses", count(host_bits, 0)),
            ]);
        } else {
            // A /31 is a point-to-point link with no network or broadcast address (RFC 3021),
            // and a /32 is one host.
            let (first, usable_last, spare) = match host_bits {
                0 | 1 => (network, last, 0),
                _ => (network + 1, last - 1, 2),
            };
            let broadcast = match host_bits {
                0 | 1 => "none".to_string(),
                _ => ip(last).to_string(),
            };
            rows.extend([
                ("broadcast", broadcast),
                ("first", ip(first).to_string()),
                ("last", ip(usable_last).to_string()),
                ("hosts", count(host_bits, spare)),
                ("netmask", ip(all & !hosts).to_string()),
                ("wildcard", ip(hosts).to_string()),
            ]);
        }
        rows.extend([
            ("prefix", format!("/{}", self.prefix)),
            ("kind", kind(self.address).to_string()),
        ]);
        rows
    }
}

/// How many bits an address of this family has.
fn width(address: IpAddr) -> u32 {
    match address {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

fn number(address: IpAddr) -> u128 {
    match address {
        IpAddr::V4(v4) => u128::from(u32::from(v4)),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

/// 2^`host_bits` less `spare`, in digits while that stays readable and as a power past that.
fn count(host_bits: u32, spare: u128) -> String {
    match host_bits {
        0..=64 => ((1u128 << host_bits) - spare).to_string(),
        bits => format!("2^{bits}"),
    }
}

/// What sort of address it is: whether it reaches the internet, and if not, why not.
fn kind(address: IpAddr) -> &'static str {
    match address {
        IpAddr::V4(v4) if v4.is_loopback() => "loopback",
        IpAddr::V4(v4) if v4.is_private() => "private (rfc 1918)",
        IpAddr::V4(v4) if v4.is_link_local() => "link-local",
        IpAddr::V4(v4) if v4.octets()[0] == 100 && v4.octets()[1] & 0xc0 == 64 => {
            "shared (carrier-grade nat)"
        }
        IpAddr::V4(v4) if v4.is_multicast() => "multicast",
        IpAddr::V6(v6) if v6.is_loopback() => "loopback",
        IpAddr::V6(v6) if v6.is_unicast_link_local() => "link-local",
        IpAddr::V6(v6) if v6.is_unique_local() => "unique local",
        IpAddr::V6(v6) if v6.is_multicast() => "multicast",
        _ => "public",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(typed: &str, label: &str) -> String {
        let rows = parse(typed).expect("a subnet").rows();
        let row = rows.into_iter().find(|(name, _)| *name == label);
        row.expect("that row").1
    }

    #[test]
    fn a_slash_22_is_worked_out() {
        let rows = parse("10.0.4.0/22").expect("a subnet").rows();
        let expected = [
            ("address", "10.0.4.0"),
            ("network", "10.0.4.0/22"),
            ("broadcast", "10.0.7.255"),
            ("first", "10.0.4.1"),
            ("last", "10.0.7.254"),
            ("hosts", "1022"),
            ("netmask", "255.255.252.0"),
            ("wildcard", "0.0.3.255"),
            ("prefix", "/22"),
            ("kind", "private (rfc 1918)"),
        ];
        let rows: Vec<(&str, &str)> = rows.iter().map(|(l, v)| (*l, v.as_str())).collect();
        assert_eq!(rows, expected);
    }

    #[test]
    fn a_host_address_finds_its_network() {
        assert_eq!(value("192.168.1.50/24", "network"), "192.168.1.0/24");
        assert_eq!(value("192.168.1.50/24", "broadcast"), "192.168.1.255");
    }

    #[test]
    fn a_mask_reads_as_its_prefix() {
        assert_eq!(value("192.168.1.0 255.255.252.0", "prefix"), "/22");
        assert_eq!(value("192.168.1.0/255.255.255.128", "prefix"), "/25");
        assert_eq!(value("192.168.1.0 0.0.0.0", "hosts"), "4294967294");
        assert!(parse("192.168.1.0 255.0.255.0").is_err());
        assert!(parse("192.168.1.0/33").is_err());
        assert!(parse("192.168.1.0").is_err());
        assert!(parse("192.168.1/24").is_err());
        assert!(parse("2001:db8:: 255.255.0.0").is_err());
    }

    #[test]
    fn point_to_point_and_single_hosts_have_no_broadcast() {
        assert_eq!(value("192.168.1.4/31", "hosts"), "2");
        assert_eq!(value("192.168.1.4/31", "first"), "192.168.1.4");
        assert_eq!(value("192.168.1.4/31", "broadcast"), "none");
        assert_eq!(value("192.168.1.4/32", "hosts"), "1");
        assert_eq!(value("192.168.1.4/32", "last"), "192.168.1.4");
        assert_eq!(value("192.168.1.4/30", "hosts"), "2");
    }

    #[test]
    fn ipv6_counts_addresses() {
        assert_eq!(value("2001:db8::1/48", "network"), "2001:db8::/48");
        assert_eq!(
            value("2001:db8::1/48", "last"),
            "2001:db8:0:ffff:ffff:ffff:ffff:ffff"
        );
        assert_eq!(value("2001:db8::/64", "addresses"), "18446744073709551616");
        assert_eq!(value("2001:db8::/48", "addresses"), "2^80");
        assert_eq!(value("::/0", "addresses"), "2^128");
        assert_eq!(value("fe80::1/64", "kind"), "link-local");
    }

    #[test]
    fn kinds_say_whether_it_reaches_the_internet() {
        assert_eq!(value("100.64.0.1/10", "kind"), "shared (carrier-grade nat)");
        assert_eq!(value("1.1.1.1/32", "kind"), "public");
        assert_eq!(value("127.0.0.1/8", "kind"), "loopback");
    }
}
