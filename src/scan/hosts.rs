//! One row of the table (an address and what the sweep learned about it), the table of them, and
//! how a typed query or a `--filter` matches a row.

use super::sweep::Event;
use ipnet::Ipv4Net;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;

/// The maker of a network card, as far as the sweep knows.
#[derive(Clone, Debug, PartialEq)]
pub enum Vendor {
    Known(String),
    Unknown,
    /// A locally administered address: a phone or laptop hiding its real MAC. No maker to look up.
    Randomized,
}

#[derive(Clone, Debug)]
pub struct Host {
    pub ip: Ipv4Addr,
    /// `aa:bb:cc:dd:ee:ff`, from the ARP table.
    pub mac: Option<String>,
    pub vendor: Vendor,
    /// What the host calls itself over mDNS (`.local`), or failing that what reverse DNS says.
    pub name: Option<String>,
    /// The ping's round trip. None when the host never answered a ping (it may still have a MAC).
    pub latency_ms: Option<f32>,
}

impl Host {
    pub fn new(ip: Ipv4Addr) -> Host {
        Host {
            ip,
            mac: None,
            vendor: Vendor::Unknown,
            name: None,
            latency_ms: None,
        }
    }

    /// Where the query matches, checking the address, then the MAC, the maker and the name.
    pub fn hit(&self, query: &Query) -> Option<Hit> {
        if query.is_empty() {
            return None;
        }
        let ip = self.ip.to_string();
        if let Some(start) = ip.find(&query.text) {
            return Some(Hit::new(Column::Ip, start, start + query.text.len()));
        }
        if query.mac_like
            && let Some(mac) = &self.mac
            && let Some(start) = mac.replace(':', "").find(&query.hex)
        {
            let end = start + query.hex.len();
            // Every two hex digits are followed by a colon in the shown form.
            return Some(Hit::new(
                Column::Mac,
                start + start / 2,
                (end - 1) + (end - 1) / 2 + 1,
            ));
        }
        if let Vendor::Known(vendor) = &self.vendor
            && let Some(hit) = text_hit(Column::Vendor, vendor, &query.text)
        {
            return Some(hit);
        }
        if let Some(name) = &self.name
            && let Some(hit) = text_hit(Column::Name, name, &query.text)
        {
            return Some(hit);
        }
        None
    }
}

/// A case-insensitive match in a maker or a name. The range is only given when lowercasing kept
/// every byte in place (true for ASCII), so it can be used to light the shown text.
fn text_hit(column: Column, shown: &str, query: &str) -> Option<Hit> {
    let lower = shown.to_lowercase();
    let start = lower.find(query)?;
    let end = start + query.len();
    if lower.len() == shown.len() && shown.is_char_boundary(start) && shown.is_char_boundary(end) {
        Some(Hit::new(column, start, end))
    } else {
        Some(Hit::new(column, 0, 0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Ip,
    Mac,
    Vendor,
    Name,
}

/// Which column a query matched and the byte range of the match in that column's shown text
/// (empty when the match can't be placed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub column: Column,
    pub start: usize,
    pub end: usize,
}

impl Hit {
    fn new(column: Column, start: usize, end: usize) -> Hit {
        Hit { column, start, end }
    }
}

/// What was typed, prepared once for matching every row.
#[derive(Clone, Debug)]
pub struct Query {
    /// Trimmed and lowercased.
    text: String,
    /// Only the hex digits, so `3d57`, `3d:57` and `3D-57` all find the same MAC.
    hex: String,
    /// Hex digits and separators only: worth checking against MACs at all.
    mac_like: bool,
}

impl Query {
    pub fn new(typed: &str) -> Query {
        let text = typed.trim().to_lowercase();
        let hex: String = text.chars().filter(char::is_ascii_hexdigit).collect();
        // Digits and dots alone are part of an IP address (`1.20`), not a switch-style MAC.
        let ip_like = text.contains('.') && text.chars().all(|c| c.is_ascii_digit() || c == '.');
        let mac_like = !hex.is_empty()
            && !ip_like
            && text
                .chars()
                .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '-' | '.'));
        Query {
            text,
            hex,
            mac_like,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// What `--filter` and the screen's prompt keep: the rows matching any of the comma-separated
/// terms. An address, a range (`192.168.1.10-20` or `192.168.1.10-192.168.1.20`) or a subnet
/// (`192.168.1.0/28`) keeps those addresses exactly; anything else is a fragment of an address, a
/// MAC (so its first bytes find a maker's devices), a maker or a name.
pub struct Filter {
    terms: Vec<Term>,
}

enum Term {
    Addresses(Ipv4Addr, Ipv4Addr),
    Fragment(Query),
}

impl Filter {
    pub fn new(typed: &str) -> Filter {
        let terms = typed
            .split(',')
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(|term| match addresses(term) {
                Some((first, last)) => Term::Addresses(first, last),
                None => Term::Fragment(Query::new(term)),
            })
            .collect();
        Filter { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn keeps(&self, host: &Host) -> bool {
        self.is_empty() || self.hit(host).is_some()
    }

    /// Where the first term that matches the row matches it. An address term lights the whole
    /// address.
    pub fn hit(&self, host: &Host) -> Option<Hit> {
        self.terms.iter().find_map(|term| match term {
            Term::Addresses(first, last) => (*first..=*last)
                .contains(&host.ip)
                .then(|| Hit::new(Column::Ip, 0, host.ip.to_string().len())),
            Term::Fragment(query) => host.hit(query),
        })
    }
}

/// The first and last address a term names, when it is an address, a range or a subnet.
fn addresses(term: &str) -> Option<(Ipv4Addr, Ipv4Addr)> {
    if let Ok(net) = term.parse::<Ipv4Net>() {
        return Some((net.network(), net.broadcast()));
    }
    if let Ok(ip) = term.parse::<Ipv4Addr>() {
        return Some((ip, ip));
    }
    let (first, last) = term.split_once('-')?;
    let first: Ipv4Addr = first.trim().parse().ok()?;
    // `192.168.1.10-20`: the end is just the last number.
    let last = match last.trim().parse::<u8>() {
        Ok(end) => {
            let [a, b, c, _] = first.octets();
            Ipv4Addr::new(a, b, c, end)
        }
        Err(_) => last.trim().parse().ok()?,
    };
    Some((first, last))
}

/// `aa:bb:cc:dd:ee:ff` from however the OS printed it: `a:b:c:d:e:f` (macOS drops leading
/// zeros), `AA-BB-CC-DD-EE-FF` (Windows), `aabb.ccdd.eeff` (switches). None for the broadcast
/// address or anything that isn't six bytes of hex.
pub fn normalize_mac(raw: &str) -> Option<String> {
    let raw = raw.trim().to_ascii_lowercase();
    let groups: Vec<&str> = raw.split([':', '-', '.']).collect();
    let hex: String = match groups.len() {
        6 => {
            let mut hex = String::with_capacity(12);
            for group in &groups {
                if group.is_empty() || group.len() > 2 {
                    return None;
                }
                hex.push_str(&format!("{group:0>2}"));
            }
            hex
        }
        3 => groups.concat(),
        1 => raw.clone(),
        _ => return None,
    };
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) || hex == "ffffffffffff" {
        return None;
    }
    let pairs: Vec<&str> = (0..12).step_by(2).map(|i| &hex[i..i + 2]).collect();
    Some(pairs.join(":"))
}

/// The text with control characters taken out. A name or an SSID is printed as it came, so
/// a host or an access point mustn't be able to slip terminal escape codes into the table.
pub fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Every host seen so far, in address order, and how far the sweep has got.
pub struct Hosts {
    pub rows: BTreeMap<Ipv4Addr, Host>,
    /// Addresses in the subnet.
    pub total: usize,
    /// Addresses pinged so far, answered or not.
    pub probed: usize,
    /// Set once the sweep has finished.
    pub took: Option<Duration>,
}

impl Hosts {
    pub fn new(total: usize) -> Hosts {
        Hosts {
            rows: BTreeMap::new(),
            total,
            probed: 0,
            took: None,
        }
    }

    pub fn done(&self) -> bool {
        self.took.is_some()
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Probed { ip, latency_ms } => {
                self.probed += 1;
                if let Some(ms) = latency_ms {
                    self.row(ip).latency_ms = Some(ms);
                }
            }
            Event::Mac { ip, mac, vendor } => {
                let row = self.row(ip);
                row.mac = Some(mac);
                row.vendor = vendor;
            }
            Event::Name { ip, name } => {
                if let Some(row) = self.rows.get_mut(&ip) {
                    row.name = Some(name);
                }
            }
            Event::Done { took } => self.took = Some(took),
        }
    }

    fn row(&mut self, ip: Ipv4Addr) -> &mut Host {
        self.rows.entry(ip).or_insert_with(|| Host::new(ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Host {
        Host {
            ip: Ipv4Addr::new(192, 168, 1, 108),
            mac: Some("2c:a5:9c:0b:3d:57".into()),
            vendor: Vendor::Known("Hangzhou Hikvision Digital Technology".into()),
            name: Some("doorcam.local".into()),
            latency_ms: Some(1.2),
        }
    }

    #[test]
    fn pads_and_lowercases_macs() {
        assert_eq!(
            normalize_mac("E6:A:2B:3C:4D:5E").as_deref(),
            Some("e6:0a:2b:3c:4d:5e")
        );
        assert_eq!(
            normalize_mac("2C-A5-9C-0B-3D-57").as_deref(),
            Some("2c:a5:9c:0b:3d:57")
        );
        assert_eq!(
            normalize_mac("2ca5.9c0b.3d57").as_deref(),
            Some("2c:a5:9c:0b:3d:57")
        );
        assert_eq!(normalize_mac("ff:ff:ff:ff:ff:ff"), None);
        assert_eq!(normalize_mac("(incomplete)"), None);
        assert_eq!(normalize_mac("2c:a5:9c:0b:3d"), None);
    }

    #[test]
    fn keeps_escape_codes_out_of_names() {
        // The escape goes; what followed it is plain text the terminal just shows.
        assert_eq!(printable("door\x1b[2Jcam\r\n"), "door[2Jcam");
        assert_eq!(printable("Café"), "Café");
    }

    #[test]
    fn finds_an_address_fragment() {
        let hit = camera().hit(&Query::new("1.10")).unwrap();
        assert_eq!(hit, Hit::new(Column::Ip, 8, 12));
        // Not the `a5:9c` in its MAC: digits and dots are an address.
        assert_eq!(camera().hit(&Query::new("5.9")), None);
    }

    #[test]
    fn finds_a_mac_with_or_without_separators() {
        let host = camera();
        let expected = Hit::new(Column::Mac, 12, 17);
        assert_eq!(host.hit(&Query::new("3d57")), Some(expected));
        assert_eq!(host.hit(&Query::new("3D:57")), Some(expected));
        assert_eq!(host.hit(&Query::new("3d-57")), Some(expected));
        assert_eq!(
            host.hit(&Query::new("0b:3d")),
            Some(Hit::new(Column::Mac, 9, 14))
        );
    }

    #[test]
    fn finds_makers_and_names_ignoring_case() {
        let host = camera();
        assert_eq!(
            host.hit(&Query::new("hikvision")),
            Some(Hit::new(Column::Vendor, 9, 18))
        );
        assert_eq!(
            host.hit(&Query::new("DOORCAM")),
            Some(Hit::new(Column::Name, 0, 7))
        );
        assert_eq!(host.hit(&Query::new("printer")), None);
    }

    #[test]
    fn an_empty_query_lights_nothing() {
        assert_eq!(camera().hit(&Query::new("")), None);
    }

    #[test]
    fn filters_by_address_range_and_subnet() {
        let keeps = |typed: &str| Filter::new(typed).keeps(&camera());
        assert!(keeps(""));
        assert!(keeps("192.168.1.108"));
        // A whole address is that address, not a fragment of .1.10x.
        assert!(!keeps("192.168.1.10"));
        assert!(keeps("192.168.1.100-110"));
        assert!(keeps("192.168.1.100 - 192.168.1.120"));
        assert!(!keeps("192.168.1.1-20"));
        assert!(keeps("192.168.1.96/28"));
        assert!(!keeps("192.168.1.0/26"));
    }

    #[test]
    fn filters_by_mac_maker_or_name_and_any_term() {
        let keeps = |typed: &str| Filter::new(typed).keeps(&camera());
        assert!(keeps("2C-A5-9C"));
        assert!(keeps("2ca59c0b3d57"));
        assert!(keeps("doorcam"));
        assert!(!keeps("aa:bb:cc,192.168.1.5"));
        assert!(keeps("aa:bb:cc,hikvision"));
        assert!(keeps("  ,hikvision"));
        assert_eq!(
            Filter::new("192.168.1.100-110").hit(&camera()),
            Some(Hit::new(Column::Ip, 0, 13))
        );
    }

    #[test]
    fn events_build_rows_in_address_order() {
        let mut hosts = Hosts::new(254);
        let later = Ipv4Addr::new(192, 168, 1, 200);
        let sooner = Ipv4Addr::new(192, 168, 1, 3);
        hosts.apply(Event::Probed {
            ip: later,
            latency_ms: Some(2.0),
        });
        hosts.apply(Event::Probed {
            ip: Ipv4Addr::new(192, 168, 1, 4),
            latency_ms: None,
        });
        hosts.apply(Event::Mac {
            ip: sooner,
            mac: "aa:bb:cc:00:11:22".into(),
            vendor: Vendor::Unknown,
        });
        hosts.apply(Event::Name {
            ip: sooner,
            name: "router.local".into(),
        });
        hosts.apply(Event::Done {
            took: Duration::from_secs(2),
        });
        let ips: Vec<Ipv4Addr> = hosts.rows.keys().copied().collect();
        assert_eq!(ips, vec![sooner, later]);
        assert_eq!(hosts.probed, 2);
        assert_eq!(hosts.rows[&sooner].name.as_deref(), Some("router.local"));
        assert!(hosts.done());
    }
}
