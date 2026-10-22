//! The maker of a network card from the first three bytes of its MAC, from the IEEE's public
//! registry (assets/oui.csv, built into the binary; see assets/README.md to refresh it).

use super::hosts::Vendor;
use std::collections::HashMap;

const REGISTRY: &str = include_str!("../../assets/oui.csv");

pub struct Vendors(HashMap<u32, String>);

impl Vendors {
    /// Reads the built-in list: tens of thousands of lines, a few milliseconds.
    pub fn load() -> Vendors {
        Vendors(parse(REGISTRY))
    }

    pub fn lookup(&self, mac: &str) -> Vendor {
        if randomized(mac) {
            return Vendor::Randomized;
        }
        match prefix(mac).and_then(|key| self.0.get(&key)) {
            Some(name) => Vendor::Known(name.clone()),
            None => Vendor::Unknown,
        }
    }
}

/// `prefix,name` lines, the name quoted when it holds a comma (a `""` inside is one quote).
pub fn parse(csv: &str) -> HashMap<u32, String> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let (prefix, name) = line.split_once(',')?;
            let name = match name.strip_prefix('"') {
                Some(quoted) => quoted
                    .strip_suffix('"')
                    .unwrap_or(quoted)
                    .replace("\"\"", "\""),
                None => name.to_string(),
            };
            Some((u32::from_str_radix(prefix, 16).ok()?, tidy(&name)))
        })
        .collect()
}

/// The first three bytes of `aa:bb:cc:dd:ee:ff` as a number.
fn prefix(mac: &str) -> Option<u32> {
    let hex: String = mac.chars().filter(|c| *c != ':').take(6).collect();
    u32::from_str_radix(&hex, 16).ok()
}

/// The "locally administered" bit (second-lowest of the first byte): set on the private Wi-Fi
/// addresses phones and laptops make up, which no registry can name.
pub fn randomized(mac: &str) -> bool {
    mac.chars()
        .nth(1)
        .and_then(|c| c.to_digit(16))
        .is_some_and(|digit| digit & 0b10 != 0)
}

/// The maker's name without the company-form tail, so it fits a column: "Apple, Inc." is Apple.
/// A tail only counts after a space, comma or period, so Cisco keeps its "co".
pub fn tidy(name: &str) -> String {
    const TAILS: [&str; 15] = [
        "co.,ltd.",
        "co., ltd.",
        "co.,ltd",
        "co., ltd",
        "corporation",
        "corp.",
        "corp",
        "limited",
        "ltd.",
        "ltd",
        "inc.",
        "inc",
        "llc",
        "co.",
        "co",
    ];
    let mut name = name.trim().to_string();
    loop {
        let lower = name.to_lowercase();
        let Some(cut) = TAILS
            .iter()
            .filter(|tail| lower.ends_with(*tail))
            .map(|tail| name.len() - tail.len())
            .find(|cut| name[..*cut].ends_with([' ', ',', '.']))
        else {
            break;
        };
        name.truncate(cut);
        name = name.trim_end_matches([' ', ',', '.']).to_string();
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_company_tails() {
        assert_eq!(tidy("Apple, Inc."), "Apple");
        assert_eq!(tidy("Ubiquiti Inc"), "Ubiquiti");
        assert_eq!(
            tidy("Hangzhou Hikvision Digital Technology Co.,Ltd."),
            "Hangzhou Hikvision Digital Technology"
        );
        assert_eq!(tidy("Cisco Systems, Inc"), "Cisco Systems");
        assert_eq!(
            tidy("Elitegroup Computer Systems Co"),
            "Elitegroup Computer Systems"
        );
        assert_eq!(tidy("Cisco"), "Cisco");
        assert_eq!(tidy("Intel Corporate"), "Intel Corporate");
        assert_eq!(tidy("Ltd."), "Ltd.");
    }

    #[test]
    fn reads_the_registry_lines() {
        let csv = "prefix,name\n\
                   001a5,Nextcomm\n\
                   2c1b3a,\"Hui Zhou Gaoshengda Technology Co.,LTD\"\n\
                   388a21,\"UAB \"\"Teltonika Telematics\"\"\"\n\
                   b8fbb3,TP-Link Systems Inc.\n";
        let makers = parse(csv);
        assert_eq!(makers[&0x2c1b3a], "Hui Zhou Gaoshengda Technology");
        assert_eq!(makers[&0x388a21], "UAB \"Teltonika Telematics\"");
        assert_eq!(makers[&0xb8fbb3], "TP-Link Systems");
        assert_eq!(makers.len(), 4);
    }

    #[test]
    fn looks_up_by_the_first_three_bytes() {
        let makers = Vendors(parse("prefix,name\nb8fbb3,TP-Link Systems Inc.\n"));
        assert_eq!(
            makers.lookup("b8:fb:b3:01:02:03"),
            Vendor::Known("TP-Link Systems".into())
        );
        assert_eq!(makers.lookup("00:11:22:01:02:03"), Vendor::Unknown);
        assert_eq!(makers.lookup("e6:0a:2b:3c:4d:5e"), Vendor::Randomized);
    }

    #[test]
    fn spots_made_up_addresses() {
        assert!(randomized("e6:0a:2b:3c:4d:5e"));
        assert!(randomized("06:11:22:33:44:55"));
        assert!(!randomized("2c:a5:9c:0b:3d:57"));
        assert!(!randomized("b8:27:eb:12:34:56"));
    }

    #[test]
    fn the_built_in_registry_reads() {
        let makers = Vendors::load();
        assert!(makers.0.len() > 30_000);
    }
}
