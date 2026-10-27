//! The networks this machine can hear, read from the OS's own tool, which needs no privileges:
//! nmcli (NetworkManager) or iw on Linux, the wifi service itself on Windows (windows.rs). macOS hides network names from
//! terminal tools, so there a small helper app (helper.swift) that the user allows Location
//! does the listing.

// On Windows only the parsers' tests use the tool-reading code: windows.rs asks the wifi service.
#![cfg_attr(windows, allow(dead_code))]

use super::channels::{self, Band};
use super::trend::Trend;
use crate::brand;
use crate::scan::hosts::{normalize_mac, printable};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The helper's program, compiled from helper.swift by build.rs. Other OSes don't use it.
#[cfg(target_os = "macos")]
const HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wifi-helper"));
#[cfg(not(target_os = "macos"))]
const HELPER: &[u8] = &[];

/// How long iwd gets to finish the fresh scan it was asked for.
const SCAN_WAIT: Duration = Duration::from_secs(3);

/// One access point as heard from here.
#[derive(Clone, Debug)]
pub struct Network {
    /// None for a hidden network.
    pub ssid: Option<String>,
    /// `aa:bb:cc:dd:ee:ff`. None when the OS didn't say.
    pub bssid: Option<String>,
    /// The centre of its primary channel.
    pub freq_mhz: u32,
    /// None where the OS doesn't say (nmcli), which then counts as 20 MHz.
    pub width_mhz: Option<u32>,
    /// nmcli gives a percentage, turned into dBm the way Windows maps them.
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
    // Windows reads its wifi service directly (windows.rs); the others ask their own tool.
    #[cfg(windows)]
    let mut networks = super::windows::networks()?;
    #[cfg(not(windows))]
    let mut networks = if cfg!(target_os = "macos") {
        macos().await?
    } else {
        linux().await?
    };
    in_order(&mut networks);
    Ok(networks)
}

/// Surveys again and again for `long`, keeping every network heard in any scan, so a weak one
/// that only answers now and then still makes the list. A scan that fails is skipped.
pub async fn survey_for(long: Duration) -> Result<Vec<Network>, String> {
    let started = Instant::now();
    let mut trend = Trend::default();
    trend.record(&survey().await?);
    while started.elapsed() < long {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Ok(latest) = survey().await {
            trend.record(&latest);
        }
    }
    Ok(trend
        .networks()
        .into_iter()
        .map(|(network, _)| network)
        .collect())
}

/// By band and channel, loudest first on each channel.
fn in_order(networks: &mut [Network]) {
    networks.sort_by_key(|network| {
        (
            Band::of(network.freq_mhz),
            network.freq_mhz,
            -network.signal_dbm,
        )
    });
}

/// NetworkManager's list where it runs (most desktops), else the kernel's last scan through iw
/// (iwd setups such as Omarchy), after asking iwd for a fresh one. iw's own scan needs root,
/// but reading the results doesn't.
async fn linux() -> Result<Vec<Network>, String> {
    // Ask for a fresh scan first; NetworkManager refuses one too soon after the last, which is fine.
    let _ = output("nmcli", &["device", "wifi", "rescan"]).await;
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

/// Runs the helper app through `open`, so macOS treats it as an app of its own (with its own
/// Location permission) rather than part of the terminal, and reads the list it writes. The
/// first run shows the Location prompt.
async fn macos() -> Result<Vec<Network>, String> {
    let app = helper_app().map_err(|why| format!("couldn't set up the wifi helper: {why}"))?;
    let list = std::env::temp_dir().join(format!("{}-wifi-{}.txt", brand::BIN, std::process::id()));
    let (app, list_arg) = (app.to_string_lossy(), list.to_string_lossy());
    output("open", &["-W", "-n", "-a", &app, "--args", &list_arg])
        .await
        .ok_or("couldn't open the wifi helper")?;
    let text = fs::read_to_string(&list).map_err(|_| "the wifi helper didn't answer")?;
    let _ = fs::remove_file(&list);
    parse_helper(&text)
}

/// The helper as an app bundle in the data folder, written (and ad-hoc signed, which macOS
/// needs before it will run it) only when this build's copy differs from what's there.
fn helper_app() -> std::io::Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", brand::CONFIG_DIR)
        .ok_or_else(|| std::io::Error::other("no data folder on this OS"))?;
    let app = dirs.data_dir().join(format!("{}.app", brand::WIFI_HELPER));
    let program = app.join("Contents/MacOS").join(brand::WIFI_HELPER);
    let plist = app.join("Contents/Info.plist");
    let info = info_plist();
    if fs::read(&program).ok().as_deref() == Some(HELPER)
        && fs::read_to_string(&plist).ok() == Some(info.clone())
    {
        return Ok(app);
    }
    fs::create_dir_all(app.join("Contents/MacOS"))?;
    fs::write(&program, HELPER)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
    }
    fs::write(&plist, info)?;
    let signed = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&app)
        .output()?;
    if !signed.status.success() {
        return Err(std::io::Error::other("codesign refused it"));
    }
    Ok(app)
}

/// The bundle's Info.plist: a background app (no Dock icon) that says why it wants Location.
fn info_plist() -> String {
    let why = format!(
        "macOS shows WiFi network names only to apps allowed Location. {} --wireless needs them \
         to name the networks it hears; it never reads where you are.",
        brand::BIN
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>{id}</string>
  <key>CFBundleName</key><string>{name}</string>
  <key>CFBundleExecutable</key><string>{name}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>LSUIElement</key><true/>
  <key>NSLocationUsageDescription</key><string>{why}</string>
  <key>NSLocationWhenInUseUsageDescription</key><string>{why}</string>
</dict>
</plist>
"#,
        id = brand::WIFI_HELPER_ID,
        name = brand::WIFI_HELPER,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// What the helper wrote: a tab-separated line per network (ssid, bssid, channel, band, width,
/// signal, `joined`), or `denied`, or `error` and why.
pub fn parse_helper(text: &str) -> Result<Vec<Network>, String> {
    if text.trim() == "denied" {
        return Err(format!(
            "macOS hides wifi names: allow {} in System Settings › Privacy & Security › Location Services, then ^r",
            brand::WIFI_HELPER
        ));
    }
    if let Some(why) = text.strip_prefix("error\t") {
        return Err(why.trim().to_string());
    }
    let mut networks = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, bssid, channel, band, width, signal, joined] = fields.as_slice() else {
            continue;
        };
        let (Ok(channel), Ok(signal)) = (channel.parse(), signal.parse()) else {
            continue;
        };
        let band = match *band {
            "2" => Some(Band::Ghz2),
            "5" => Some(Band::Ghz5),
            "6" => Some(Band::Ghz6),
            _ => None,
        };
        networks.push(Network {
            ssid: ssid(name),
            bssid: normalize_mac(bssid),
            freq_mhz: on_channel(channel, band),
            width_mhz: width.parse().ok().filter(|mhz| *mhz > 0),
            signal_dbm: signal,
            connected: *joined == "joined",
        });
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

pub fn ssid(raw: &str) -> Option<String> {
    let raw = printable(raw.trim());
    // iw prints a hidden network's name as zero bytes, `\x00\x00...`.
    (!raw.is_empty() && !raw.starts_with("\\x00")).then_some(raw)
}

/// A beacon's channel width from its information elements (id, length, body): 40 MHz when the
/// HT operation element (61) names a secondary channel, 80 or 160 from the VHT operation element
/// (192). None when neither says. Only windows.rs reads beacons; it's tested here on every OS.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn width_from_ies(mut ies: &[u8]) -> Option<u32> {
    let mut width = None;
    while let [id, length, rest @ ..] = ies {
        let Some(body) = rest.get(..*length as usize) else {
            break;
        };
        match (id, body) {
            (61, [_, info, ..]) if matches!(info & 0b11, 1 | 3) => width = width.max(Some(40)),
            // Width 1 is 80 MHz, or 160 when a second centre segment is set; 2 and 3 are 160.
            (192, [1, _, 0, ..]) => width = width.max(Some(80)),
            (192, [1..=3, ..]) => width = width.max(Some(160)),
            _ => {}
        }
        ies = &rest[*length as usize..];
    }
    width
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_width_from_beacon_elements() {
        let ht_40 = [61, 3, 36, 0b0101, 0];
        let vht_80 = [192, 3, 1, 42, 0];
        let vht_160 = [192, 3, 1, 42, 50];
        let other = [0, 4, b'H', b'o', b'm', b'e'];
        assert_eq!(
            width_from_ies(&[other.as_slice(), &ht_40].concat()),
            Some(40)
        );
        assert_eq!(
            width_from_ies(&[ht_40.as_slice(), &vht_80].concat()),
            Some(80)
        );
        assert_eq!(
            width_from_ies(&[ht_40.as_slice(), &vht_160].concat()),
            Some(160)
        );
        assert_eq!(width_from_ies(&other), None);
        assert_eq!(
            width_from_ies(&[61, 9, 1]),
            None,
            "a length running past the end"
        );
    }

    #[test]
    fn reads_nmcli() {
        let text = "*:AA\\:BB\\:CC\\:00\\:00\\:01:Home\\:Net:2437 MHz:70\n \
                    :AA\\:BB\\:CC\\:00\\:00\\:02::5180 MHz:40\n \
                    :AA\\:BB\\:CC\\:00\\:00\\:03:Cafe\x1b[2J:2412 MHz:50\n";
        let networks = parse_nmcli(text);
        assert_eq!(networks.len(), 3);
        assert_eq!(networks[2].ssid.as_deref(), Some("Cafe[2J"));
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
    fn reads_the_macos_helper() {
        let text = "Home\taa:bb:cc:00:00:01\t149\t5\t80\t-54\tjoined\n\
                    \taa:bb:cc:00:00:02\t6\t2\t0\t-71\t\n\
                    Attic\t\t37\t6\t160\t-80\t\n";
        let networks = parse_helper(text).unwrap();
        assert_eq!(networks.len(), 3);
        assert_eq!(networks[0].ssid.as_deref(), Some("Home"));
        assert!(networks[0].connected);
        assert_eq!(networks[0].freq_mhz, 5745);
        assert_eq!(networks[0].width_mhz, Some(80));
        assert_eq!(networks[1].ssid, None);
        assert_eq!(networks[1].freq_mhz, 2437);
        assert_eq!(networks[1].width_mhz, None);
        assert_eq!(networks[1].signal_dbm, -71);
        assert_eq!(networks[2].bssid, None);
        assert_eq!(networks[2].freq_mhz, 6135);
        assert!(
            parse_helper("denied\n")
                .unwrap_err()
                .contains("Location Services")
        );
        assert_eq!(
            parse_helper("error\tno wifi card found\n").unwrap_err(),
            "no wifi card found"
        );
    }
}
