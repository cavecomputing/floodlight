//! The networks this machine can hear, read from the OS's own tool, which needs no privileges:
//! nmcli (NetworkManager) or iw on Linux, system_profiler on macOS, netsh on Windows.

use super::channels::{self, Band};
use crate::scan::hosts::normalize_mac;
use std::time::Duration;

/// How long iwd gets to finish the fresh scan it was asked for.
const SCAN_WAIT: Duration = Duration::from_secs(3);

/// One access point as heard from here.
#[derive(Clone, Debug)]
pub struct Network {
    /// None for a hidden network.
    pub ssid: Option<String>,
    /// `aa:bb:cc:dd:ee:ff`. None on macOS, which doesn't say.
    pub bssid: Option<String>,
    /// The centre of its primary channel.
    pub freq_mhz: u32,
    /// None where the OS doesn't say (nmcli, netsh), which then counts as 20 MHz.
    pub width_mhz: Option<u32>,
    /// nmcli and netsh give a percentage, turned into dBm the way Windows maps them.
    pub signal_dbm: i32,
    /// The one this machine is joined to.
    pub connected: bool,
}

impl Network {
    pub fn span(&self) -> (u32, u32) {
        channels::span(self.freq_mhz, self.width_mhz)
    }
}

/// Every network heard, by band and channel, loudest first on each channel. The error says
/// what's missing in a few words.
pub async fn survey() -> Result<Vec<Network>, String> {
    let mut networks = if cfg!(target_os = "macos") {
        let text = output("system_profiler", &["SPAirPortDataType"])
            .await
            .ok_or("system_profiler couldn't list networks")?;
        parse_system_profiler(&text)
    } else if cfg!(windows) {
        let text = output("netsh", &["wlan", "show", "networks", "mode=bssid"])
            .await
            .ok_or("netsh couldn't list networks: is wifi on?")?;
        parse_netsh(&text)
    } else {
        linux().await?
    };
    networks.sort_by_key(|network| {
        (
            Band::of(network.freq_mhz),
            network.freq_mhz,
            -network.signal_dbm,
        )
    });
    Ok(networks)
}

/// NetworkManager's list where it runs (most desktops), else the kernel's last scan through iw
/// (iwd setups such as Omarchy), after asking iwd for a fresh one. iw's own scan needs root,
/// but reading the results doesn't.
async fn linux() -> Result<Vec<Network>, String> {
    let fields = "IN-USE,BSSID,SSID,FREQ,SIGNAL";
    if let Some(text) = output("nmcli", &["-t", "-f", fields, "device", "wifi", "list"]).await {
        return Ok(parse_nmcli(&text));
    }
    let interfaces: Vec<String> = std::fs::read_dir("/sys/class/net")
        .map_err(|why| why.to_string())?
        .flatten()
        .filter(|entry| entry.path().join("wireless").exists())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    if interfaces.is_empty() {
        return Err("no wifi card found".to_string());
    }
    let mut networks = Vec::new();
    for interface in interfaces {
        if output("iwctl", &["station", &interface, "scan"])
            .await
            .is_some()
        {
            tokio::time::sleep(SCAN_WAIT).await;
        }
        let text = output("iw", &["dev", &interface, "scan", "dump"])
            .await
            .ok_or("listing networks needs nmcli (NetworkManager) or iw")?;
        networks.extend(parse_iw(&text));
    }
    Ok(networks)
}

/// The program's output, when it ran and succeeded.
async fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn ssid(raw: &str) -> Option<String> {
    let raw = raw.trim();
    // iw prints a hidden network's name as zero bytes, `\x00\x00...`.
    (!raw.is_empty() && !raw.starts_with("\\x00")).then(|| raw.to_string())
}

fn percent_to_dbm(percent: i32) -> i32 {
    percent.clamp(0, 100) / 2 - 100
}

/// A channel number the OS gave without its band: 1 to 14 are 2.4 GHz, the rest 5 GHz.
fn on_channel(channel: u32, band: Option<Band>) -> u32 {
    let band = band.unwrap_or(if channel <= 14 {
        Band::Ghz2
    } else {
        Band::Ghz5
    });
    channels::frequency(band, channel)
}

/// `nmcli -t`: one network a line, fields split by `:`, a `:` inside a field written `\:`.
/// `*:AA\:BB\:CC\:DD\:EE\:01:Home:2437 MHz:70`
pub fn parse_nmcli(text: &str) -> Vec<Network> {
    let mut networks = Vec::new();
    for line in text.lines() {
        let mut fields = vec![String::new()];
        let mut escaped = false;
        for c in line.chars() {
            match c {
                '\\' if !escaped => {
                    escaped = true;
                    continue;
                }
                ':' if !escaped => fields.push(String::new()),
                _ => fields.last_mut().expect("starts with one").push(c),
            }
            escaped = false;
        }
        let [in_use, bssid, name, freq, signal] = fields.as_slice() else {
            continue;
        };
        let (Some(freq), Ok(signal)) = (
            freq.split_whitespace()
                .next()
                .and_then(|mhz| mhz.parse().ok()),
            signal.parse(),
        ) else {
            continue;
        };
        networks.push(Network {
            ssid: ssid(name),
            bssid: normalize_mac(bssid),
            freq_mhz: freq,
            width_mhz: None,
            signal_dbm: percent_to_dbm(signal),
            connected: in_use.trim() == "*",
        });
    }
    networks
}

/// `iw dev wlan0 scan dump`: a `BSS` line per access point, then indented details. The width is
/// 40 MHz when HT names a secondary channel, more when VHT says so.
pub fn parse_iw(text: &str) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("BSS ") {
            networks.push(Network {
                ssid: None,
                bssid: normalize_mac(rest.split('(').next().unwrap_or(rest)),
                freq_mhz: 0,
                width_mhz: None,
                signal_dbm: -100,
                connected: rest.contains("associated"),
            });
            continue;
        }
        let Some(network) = networks.last_mut() else {
            continue;
        };
        let line = line.trim();
        let number = |value: &str| -> Option<f32> { value.split_whitespace().next()?.parse().ok() };
        if let Some(value) = line.strip_prefix("freq:") {
            network.freq_mhz = number(value).unwrap_or(0.0) as u32;
        } else if let Some(value) = line.strip_prefix("signal:") {
            network.signal_dbm = number(value).unwrap_or(-100.0).round() as i32;
        } else if let Some(value) = line.strip_prefix("SSID:") {
            network.ssid = ssid(value);
        } else if let Some(value) = line.strip_prefix("* secondary channel offset:") {
            if value.contains("above") || value.contains("below") {
                network.width_mhz = Some(40);
            }
        } else if let Some(value) = line.strip_prefix("* channel width:") {
            match number(value) {
                Some(1.0) => network.width_mhz = Some(80),
                Some(2.0) | Some(3.0) => network.width_mhz = Some(160),
                _ => {}
            }
        }
    }
    networks.retain(|network| network.freq_mhz > 0);
    networks
}

/// `system_profiler SPAirPortDataType`: the joined network under `Current Network Information:`,
/// the rest under `Other Local Wi-Fi Networks:`, each a name line ending in `:` and then
/// `Channel: 149 (5GHz, 80MHz)` and `Signal / Noise: -54 dBm / -95 dBm`.
pub fn parse_system_profiler(text: &str) -> Vec<Network> {
    let mut networks = Vec::new();
    // The heading's indent and whether it's the joined network.
    let mut section: Option<(usize, bool)> = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((heading, _)) = section
            && indent <= heading
        {
            section = None;
        }
        match line {
            "Current Network Information:" => section = Some((indent, true)),
            "Other Local Wi-Fi Networks:" => section = Some((indent, false)),
            _ => {}
        }
        let Some((_, connected)) = section else {
            continue;
        };
        if let Some(name) = line.strip_suffix(':') {
            networks.push(Network {
                ssid: ssid(name),
                bssid: None,
                freq_mhz: 0,
                width_mhz: None,
                signal_dbm: -100,
                connected,
            });
        } else if let Some(network) = networks.last_mut() {
            if let Some(value) = line.strip_prefix("Channel:") {
                let numbers: Vec<u32> = value
                    .split(|c: char| !c.is_ascii_digit())
                    .filter_map(|number| number.parse().ok())
                    .collect();
                let band = match numbers.get(1) {
                    Some(2) => Some(Band::Ghz2),
                    Some(5) => Some(Band::Ghz5),
                    Some(6) => Some(Band::Ghz6),
                    _ => None,
                };
                if let Some(channel) = numbers.first() {
                    network.freq_mhz = on_channel(*channel, band);
                }
                network.width_mhz = numbers.get(2).copied();
            } else if let Some(value) = line.strip_prefix("Signal / Noise:") {
                network.signal_dbm = value
                    .split_whitespace()
                    .next()
                    .and_then(|dbm| dbm.parse().ok())
                    .unwrap_or(-100);
            }
        }
    }
    networks.retain(|network| network.freq_mhz > 0);
    networks
}

/// `netsh wlan show networks mode=bssid`: an `SSID 1 : name` line, then a `BSSID 1 : mac` line
/// per access point with its `Signal : 90%`, `Channel : 36` and, on newer Windows,
/// `Band : 5 GHz`. It doesn't say which one is joined.
pub fn parse_netsh(text: &str) -> Vec<Network> {
    let mut heard: Vec<(Network, u32, Option<Band>)> = Vec::new();
    let mut name = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.starts_with("SSID") {
            name = ssid(value);
        } else if key.starts_with("BSSID") {
            let network = Network {
                ssid: name.clone(),
                bssid: normalize_mac(value),
                freq_mhz: 0,
                width_mhz: None,
                signal_dbm: -100,
                connected: false,
            };
            heard.push((network, 0, None));
        } else if let Some((network, channel, band)) = heard.last_mut() {
            match key {
                "Signal" => {
                    let percent = value.trim_end_matches('%').parse().unwrap_or(0);
                    network.signal_dbm = percent_to_dbm(percent);
                }
                "Channel" => *channel = value.parse().unwrap_or(0),
                "Band" if value.starts_with('2') => *band = Some(Band::Ghz2),
                "Band" if value.starts_with('5') => *band = Some(Band::Ghz5),
                "Band" if value.starts_with('6') => *band = Some(Band::Ghz6),
                _ => {}
            }
        }
    }
    heard
        .into_iter()
        .filter(|(_, channel, _)| *channel > 0)
        .map(|(network, channel, band)| Network {
            freq_mhz: on_channel(channel, band),
            ..network
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nmcli() {
        let text = "*:AA\\:BB\\:CC\\:00\\:00\\:01:Home\\:Net:2437 MHz:70\n \
                    :AA\\:BB\\:CC\\:00\\:00\\:02::5180 MHz:40\n";
        let networks = parse_nmcli(text);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid.as_deref(), Some("Home:Net"));
        assert_eq!(networks[0].bssid.as_deref(), Some("aa:bb:cc:00:00:01"));
        assert_eq!(networks[0].freq_mhz, 2437);
        assert_eq!(networks[0].signal_dbm, -65);
        assert!(networks[0].connected);
        assert_eq!(networks[1].ssid, None);
        assert!(!networks[1].connected);
    }

    #[test]
    fn reads_iw() {
        let text = "\
BSS aa:bb:cc:00:00:01(on wlan0) -- associated
\tfreq: 5180.0
\tsignal: -48.00 dBm
\tSSID: Home
\tHT operation:
\t\t * primary channel: 36
\t\t * secondary channel offset: above
\t\t * STA channel width: any
\tVHT operation:
\t\t * channel width: 1 (80 MHz)
\t\t * center freq segment 1: 42
BSS aa:bb:cc:00:00:02(on wlan0)
\tfreq: 2412
\tsignal: -71.00 dBm
\tSSID: \\x00\\x00\\x00
\tHT operation:
\t\t * secondary channel offset: no secondary
";
        let networks = parse_iw(text);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].freq_mhz, 5180);
        assert_eq!(networks[0].width_mhz, Some(80));
        assert_eq!(networks[0].signal_dbm, -48);
        assert!(networks[0].connected);
        assert_eq!(networks[1].ssid, None);
        assert_eq!(networks[1].width_mhz, None);
        assert_eq!(networks[1].bssid.as_deref(), Some("aa:bb:cc:00:00:02"));
    }

    #[test]
    fn reads_system_profiler() {
        let text = "\
Wi-Fi:

      Interfaces:
        en0:
          Card Type: Wi-Fi
          Status: Connected
          Current Network Information:
            Home:
              PHY Mode: 802.11ac
              Channel: 149 (5GHz, 80MHz)
              Signal / Noise: -54 dBm / -95 dBm
          Other Local Wi-Fi Networks:
            Neighbour:
              Channel: 6 (2GHz, 20MHz)
              Signal / Noise: -71 dBm / -95 dBm
            Attic:
              Channel: 37 (6GHz, 160MHz)
              Signal / Noise: -80 dBm / -95 dBm
        awdl0:
          Status: Off
";
        let networks = parse_system_profiler(text);
        assert_eq!(networks.len(), 3);
        assert_eq!(networks[0].ssid.as_deref(), Some("Home"));
        assert!(networks[0].connected);
        assert_eq!(networks[0].freq_mhz, 5745);
        assert_eq!(networks[0].width_mhz, Some(80));
        assert_eq!(networks[1].freq_mhz, 2437);
        assert_eq!(networks[1].signal_dbm, -71);
        assert!(!networks[1].connected);
        assert_eq!(networks[2].freq_mhz, 6135);
    }

    #[test]
    fn reads_netsh() {
        let text = "\
Interface name : Wi-Fi
There are 2 networks currently visible.

SSID 1 : Home
    Network type            : Infrastructure
    Authentication          : WPA2-Personal
    BSSID 1                 : aa:bb:cc:00:00:01
         Signal             : 90%
         Radio type         : 802.11ax
         Band               : 6 GHz
         Channel            : 5
    BSSID 2                 : aa:bb:cc:00:00:02
         Signal             : 40%
         Channel            : 11

SSID 2 : 
    BSSID 1                 : aa:bb:cc:00:00:03
         Signal             : 60%
         Channel            : 44
";
        let networks = parse_netsh(text);
        assert_eq!(networks.len(), 3);
        assert_eq!(networks[0].ssid.as_deref(), Some("Home"));
        assert_eq!(networks[0].freq_mhz, 5975);
        assert_eq!(networks[0].signal_dbm, -55);
        assert_eq!(networks[1].freq_mhz, 2462);
        assert_eq!(networks[2].ssid, None);
        assert_eq!(networks[2].freq_mhz, 5220);
    }
}
