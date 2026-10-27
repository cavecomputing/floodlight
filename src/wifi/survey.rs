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
    /// The centre of the whole bonded channel, where the beacon said (iw, Windows): a 40 MHz
    /// network on 2.4 GHz channel 6 is centred 10 MHz above or below it. None where only the
    /// width is known (macOS) or nothing is (nmcli); the slice is then guessed from the width.
    pub centre_mhz: Option<u32>,
    /// nmcli gives a percentage, turned into dBm the way Windows maps them.
    pub signal_dbm: i32,
    /// The one this machine is joined to.
    pub connected: bool,
}

impl Network {
    pub fn span(&self) -> (u32, u32) {
        channels::span(self.freq_mhz, self.width_mhz, self.centre_mhz)
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
    // Signing rewrites the program inside the bundle, so it never matches this build's bytes
    // again. A copy of what was written is kept beside the bundle to compare with instead.
    let written = dirs
        .data_dir()
        .join(format!("{}.written", brand::WIFI_HELPER));
    let info = info_plist();
    if fs::read(&written).ok().as_deref() == Some(HELPER)
        && program.exists()
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
    fs::write(&written, HELPER)?;
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
            centre_mhz: None,
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

/// The name to show, None for a hidden network (no name, or only zero bytes, which
/// `printable` drops).
pub fn ssid(raw: &str) -> Option<String> {
    let raw = printable(raw.trim());
    (!raw.is_empty()).then_some(raw)
}

/// iw's name with its escapes undone: it writes every byte that isn't plain ASCII, and a
/// backslash, as `\xNN`, so `Café` arrives as `Caf\xc3\xa9` and a hidden name as `\x00\x00`.
fn unescape_iw(escaped: &str) -> String {
    let mut bytes = Vec::new();
    let mut rest = escaped.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let code = match (byte, after) {
            (b'\\', [b'x', high, low, ..]) => {
                u8::from_str_radix(&format!("{}{}", *high as char, *low as char), 16).ok()
            }
            _ => None,
        };
        match code {
            Some(code) => {
                bytes.push(code);
                rest = &after[3..];
            }
            None => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A beacon's channel width and the centre of its bonded channel, from its information
/// elements (id, length, body), given the primary channel's frequency: 20 MHz on the primary
/// unless the HT operation element (61) names a secondary channel (40 MHz, above or below), the
/// VHT operation element (192) says 80 or 160, the HE operation element (255, extension 36) says
/// so for 6 GHz, or the EHT operation element (255, extension 106) says up to 320. Each names the
/// centre as a channel number; a 160 or 320 MHz one in its second segment field. Only windows.rs
/// reads beacons; it's tested here on every OS.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn bond_from_ies(mut ies: &[u8], freq_mhz: u32) -> (u32, u32) {
    let band = Band::of(freq_mhz);
    let mut bond = (20, freq_mhz);
    while let [id, length, rest @ ..] = ies {
        let Some(body) = rest.get(..*length as usize) else {
            break;
        };
        match (id, body) {
            (61, [_, info, ..]) => match info & 0b11 {
                1 => bond = (40, freq_mhz + 10),
                3 => bond = (40, freq_mhz - 10),
                _ => {}
            },
            // Width 1 is 80 MHz, or 160 when the second segment is the 160's centre (80+80, two
            // separate 80s, is drawn as the first); 2 is 160 around the first segment; 3 is 80+80.
            (192, [1, seg0, seg1, ..]) if *seg1 != 0 && seg1.abs_diff(*seg0) == 8 => {
                bond = (160, channels::frequency(band, *seg1 as u32));
            }
            (192, [1 | 3, seg0, ..]) => bond = (80, channels::frequency(band, *seg0 as u32)),
            (192, [2, seg0, ..]) => bond = (160, channels::frequency(band, *seg0 as u32)),
            // Parameters (3), colour, MCS set (2), optional parts, then the 6 GHz information
            // (primary, control, two segments, rate) when the parameters' bit 17 says so.
            (255, [36, params @ .., _, control, seg0, seg1, _])
                if params.len() >= 6 && params[2] & 0b10 != 0 =>
            {
                bond = bonded(control & 0b11, *seg0, *seg1, band);
            }
            (255, [106, params, _, _, _, _, control, seg0, seg1, ..]) if params & 1 != 0 => {
                bond = bonded(control & 0b111, *seg0, *seg1, band);
            }
            _ => {}
        }
        ies = &rest[*length as usize..];
    }
    bond
}

/// A width code (0 for 20 MHz, 1 for 40, up to 4 for 320) and the two centre segments the HE and
/// EHT operation elements carry, as a width and its centre.
fn bonded(code: u8, seg0: u8, seg1: u8, band: Band) -> (u32, u32) {
    let width = 20 << code.min(4);
    let segment = if width >= 160 && seg1 != 0 {
        seg1
    } else {
        seg0
    };
    (width, channels::frequency(band, segment as u32))
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
            centre_mhz: None,
            signal_dbm: percent_to_dbm(signal),
            connected: in_use.trim() == "*",
        });
    }
    networks
}

/// `iw dev wlan0 scan dump`: a `BSS` line per access point, then indented details. The width and
/// the bonded channel's centre come from the HT, VHT, HE (6 GHz) and EHT operation elements iw
/// prints, the way `bond_from_ies` reads them from the bytes.
pub fn parse_iw(text: &str) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("BSS ") {
            networks.push(Network {
                ssid: None,
                bssid: normalize_mac(rest.split('(').next().unwrap_or(rest)),
                freq_mhz: 0,
                width_mhz: Some(20),
                centre_mhz: None,
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
        let band = Band::of(network.freq_mhz);
        // A centre as a channel number; 0 means none.
        let segment = |value: &str| {
            number(value)
                .filter(|index| *index > 0.0)
                .map(|index| channels::frequency(band, index as u32))
        };
        if let Some(value) = line.strip_prefix("freq:") {
            network.freq_mhz = number(value).unwrap_or(0.0) as u32;
            network.centre_mhz = Some(network.freq_mhz);
        } else if let Some(value) = line.strip_prefix("signal:") {
            network.signal_dbm = number(value).unwrap_or(-100.0).round() as i32;
        } else if let Some(value) = line.strip_prefix("SSID:") {
            network.ssid = ssid(&unescape_iw(value));
        } else if let Some(value) = line.strip_prefix("* secondary channel offset:") {
            if value.contains("above") {
                network.width_mhz = Some(40);
                network.centre_mhz = Some(network.freq_mhz + 10);
            } else if value.contains("below") {
                network.width_mhz = Some(40);
                network.centre_mhz = Some(network.freq_mhz - 10);
            }
        } else if let Some(value) = line.strip_prefix("* channel width:") {
            // VHT: 1 is 80 (or 160, which its second segment then says), 2 is 160, 3 is 80+80.
            match number(value) {
                Some(1.0) | Some(3.0) => network.width_mhz = Some(80),
                Some(2.0) => network.width_mhz = Some(160),
                _ => {}
            }
        } else if let Some(value) = line.strip_prefix("Channel Width:") {
            // HE (6 GHz) and EHT: `80 MHz`, `80+80 or 160 MHz`, `320 MHz`.
            let last = value
                .split_whitespace()
                .rev()
                .nth(1)
                .and_then(|mhz| mhz.parse().ok());
            if let Some(mhz) = last {
                network.width_mhz = Some(mhz);
            }
        } else if let Some(value) = line
            .strip_prefix("* center freq segment 1:")
            .or_else(|| line.strip_prefix("Center Frequency Segment 0:"))
        {
            network.centre_mhz = segment(value).or(network.centre_mhz);
        } else if let Some(value) = line
            .strip_prefix("* center freq segment 2:")
            .or_else(|| line.strip_prefix("Center Frequency Segment 1:"))
        {
            // The second segment is the centre of a 160 or 320 MHz channel. VHT's `1 (80 MHz)`
            // with a second segment 8 channels off is really 160 MHz.
            if let (Some(mhz), Some(centre)) = (segment(value), network.centre_mhz)
                && (network.width_mhz >= Some(160) || mhz.abs_diff(centre) == 40)
            {
                network.width_mhz = network.width_mhz.max(Some(160));
                network.centre_mhz = Some(mhz);
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
    fn reads_the_bonded_channel_from_beacon_elements() {
        let ht_40_below = [61, 3, 11, 0b0111, 0];
        let ht_40 = [61, 3, 36, 0b0101, 0];
        let ht_20 = [61, 3, 36, 0b0100, 0];
        let vht_80 = [192, 5, 1, 42, 0, 0, 0];
        let vht_160 = [192, 5, 1, 42, 50, 0, 0];
        let vht_80_80 = [192, 5, 1, 42, 155, 0, 0];
        let vht_160_old = [192, 5, 2, 50, 0, 0, 0];
        // Extension 36, parameters with bit 17 set, colour, MCS set, then the 6 GHz information:
        // primary 37, width code 3 (160), segments 39 (the 80) and 47 (the 160).
        let he_6ghz_160 = [255, 12, 36, 0, 0, 0b10, 0, 0xff, 0xff, 37, 3, 39, 47, 6];
        let he_5ghz = [255, 7, 36, 0, 0, 0, 0, 0xff, 0xff];
        // Extension 106, parameters with the information present, MCS set, then width code 4
        // (320) with segments 47 (the 160) and 63 (the 320).
        let eht_320 = [255, 9, 106, 1, 0x11, 0, 0, 0, 4, 47, 63];
        let other = [0, 4, b'H', b'o', b'm', b'e'];
        assert_eq!(bond_from_ies(&other, 2437), (20, 2437));
        assert_eq!(bond_from_ies(&ht_20, 5180), (20, 5180));
        assert_eq!(
            bond_from_ies(&[other.as_slice(), &ht_40].concat(), 5180),
            (40, 5190)
        );
        assert_eq!(bond_from_ies(&ht_40_below, 2462), (40, 2452));
        assert_eq!(
            bond_from_ies(&[ht_40.as_slice(), &vht_80].concat(), 5180),
            (80, 5210)
        );
        assert_eq!(
            bond_from_ies(&[ht_40.as_slice(), &vht_160].concat(), 5180),
            (160, 5250)
        );
        assert_eq!(bond_from_ies(&vht_80_80, 5180), (80, 5210));
        assert_eq!(bond_from_ies(&vht_160_old, 5180), (160, 5250));
        assert_eq!(bond_from_ies(&he_6ghz_160, 6135), (160, 6185));
        assert_eq!(
            bond_from_ies(&[he_5ghz.as_slice(), &vht_80].concat(), 5180),
            (80, 5210)
        );
        assert_eq!(
            bond_from_ies(&[he_6ghz_160.as_slice(), &eht_320].concat(), 6135),
            (320, 6265)
        );
        assert_eq!(
            bond_from_ies(&[61, 9, 1], 5180),
            (20, 5180),
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
BSS aa:bb:cc:00:00:03(on wlan0)
\tfreq: 2462
\tSSID: Caf\\xc3\\xa9 Attic
\tHT operation:
\t\t * primary channel: 11
\t\t * secondary channel offset: below
BSS aa:bb:cc:00:00:04(on wlan0)
\tfreq: 6135.0
\tSSID: Attic
\tHE Operation:
\t\tHE Operation Parameters: (0x020000)
\t\t\t6 GHz Operation Information Present
\t\t6 GHz Operation Information: 0x2503272f06
\t\t\tPrimary Channel: 37
\t\t\tChannel Width: 80+80 or 160 MHz
\t\t\tCenter Frequency Segment 0: 39
\t\t\tCenter Frequency Segment 1: 47
\tEHT Operation:
\t\tEHT Operation Parameters: (0x01)
\t\tEHT Operation Info: 0x042f3f
\t\t\tChannel Width: 320 MHz
\t\t\tCenter Frequency Segment 0: 47
\t\t\tCenter Frequency Segment 1: 63
BSS aa:bb:cc:00:00:05(on wlan0)
\tfreq: 5500
\tVHT operation:
\t\t * channel width: 1 (80 MHz)
\t\t * center freq segment 1: 106
\t\t * center freq segment 2: 114
";
        let networks = parse_iw(text);
        assert_eq!(networks.len(), 5);
        assert_eq!(networks[0].freq_mhz, 5180);
        assert_eq!(networks[0].width_mhz, Some(80));
        assert_eq!(networks[0].centre_mhz, Some(5210));
        assert_eq!(networks[0].signal_dbm, -48);
        assert!(networks[0].connected);
        assert_eq!(networks[1].ssid, None);
        assert_eq!(networks[1].width_mhz, Some(20));
        assert_eq!(networks[1].centre_mhz, Some(2412));
        assert_eq!(networks[1].bssid.as_deref(), Some("aa:bb:cc:00:00:02"));
        assert_eq!(networks[2].ssid.as_deref(), Some("Café Attic"));
        assert_eq!(networks[2].width_mhz, Some(40));
        assert_eq!(networks[2].centre_mhz, Some(2452));
        assert_eq!(networks[3].width_mhz, Some(320));
        assert_eq!(networks[3].centre_mhz, Some(6265));
        assert_eq!(networks[4].width_mhz, Some(160));
        assert_eq!(networks[4].centre_mhz, Some(5570));
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
