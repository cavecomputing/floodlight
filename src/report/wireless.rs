//! The WiFi survey as a report: the network this machine is on and how crowded its channel is,
//! the quietest channels, a chart of each band, and a table of every network heard.

use super::pdf::{self, Font};
use super::sheet::{self, AQUA, BORDER, COOL, Cell, Column, FG, LEFT, MUTED, SUBTLE, Sheet, WIDTH};
use crate::ui::plain;
use crate::wifi::channels::{self, Band};
use crate::wifi::survey::{self, Network};
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;

const COLUMNS: [Column; 6] = [
    Column::text("Network", 0.27),
    Column::number("Ch", 0.06),
    Column::text("Band", 0.10),
    Column::text("Width", 0.10),
    Column::number("Signal", 0.10),
    Column::mono("BSSID", 0.37),
];

/// Lanes in one band's chart; quieter networks past this are left out (the table has them all).
const MOST_LANES: usize = 48;
const LANE: f32 = 10.0;
/// The chart's left column, where the network names go.
const NAMES: f32 = 130.0;

/// Scans for `seconds` (weak networks often turn up only in the second or third scan), then
/// writes the report.
pub fn write(runtime: &Runtime, path: &Path, seconds: u64) -> anyhow::Result<()> {
    println!("scanning for {seconds} s");
    let networks = runtime
        .block_on(survey::survey_for(Duration::from_secs(seconds)))
        .map_err(anyhow::Error::msg)?;
    std::fs::write(path, document(&networks))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The report's PDF file.
pub fn document(networks: &[Network]) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(&date, &time);

    let bands: Vec<Band> = [Band::Ghz2, Band::Ghz5, Band::Ghz6]
        .into_iter()
        .filter(|band| *band != Band::Ghz6 || heard_on(networks, *band))
        .collect();
    let mut facts = vec![("date", date), ("time", time)];
    for band in &bands {
        let channel = channels::quietest(networks, *band);
        facts.push(("quietest", format!("{channel} on {}", band.name())));
    }
    let joined = networks.iter().position(|network| network.connected);
    let subtitle = match joined {
        Some(i) => format!(
            "joined {} on channel {} · {}",
            name(&networks[i]),
            channels::channel(networks[i].freq_mhz),
            Band::of(networks[i].freq_mhz).name()
        ),
        None => "not joined to a network".to_string(),
    };
    let title = format!("{} networks heard", networks.len());
    sheet.masthead("Wireless survey", &title, &subtitle, &facts);
    sheet.summary(&summary(networks, joined, &bands));

    for band in bands.iter().filter(|band| heard_on(networks, **band)) {
        let on_band: Vec<&Network> = networks
            .iter()
            .filter(|network| Band::of(network.freq_mhz) == *band)
            .collect();
        chart(
            &mut sheet,
            *band,
            &on_band,
            channels::quietest(networks, *band),
        );
    }

    sheet.heading("Networks", 36.0);
    let rows: Vec<Vec<Cell>> = networks
        .iter()
        .map(|network| {
            let words = plain::network_words(network);
            let ink = |i: usize| match i {
                0 if network.connected => AQUA,
                0 => FG,
                5 => COOL,
                _ => MUTED,
            };
            words
                .into_iter()
                .enumerate()
                .map(|(i, word)| (word, ink(i)))
                .collect()
        })
        .collect();
    if rows.is_empty() {
        sheet.summary("Nothing heard.");
    } else {
        sheet.table(&COLUMNS, &rows);
    }
    sheet.finish()
}

fn heard_on(networks: &[Network], band: Band) -> bool {
    networks
        .iter()
        .any(|network| Band::of(network.freq_mhz) == band)
}

fn name(network: &Network) -> &str {
    network.ssid.as_deref().unwrap_or("hidden")
}

/// How crowded the joined network's channel is, and where it would be quieter.
fn summary(networks: &[Network], joined: Option<usize>, bands: &[Band]) -> String {
    let quietest: Vec<String> = bands
        .iter()
        .map(|band| format!("{} on {}", channels::quietest(networks, *band), band.name()))
        .collect();
    let quietest = format!("The quietest channels here are {}.", quietest.join(" and "));
    let Some(i) = joined else {
        return format!("{} networks heard. {quietest}", networks.len());
    };
    let network = &networks[i];
    let crowd = channels::crowds(networks)[i];
    let plural = |n: usize| if n == 1 { "network" } else { "networks" };
    let crowded = match (crowd.same, crowd.overlapping) {
        (0, 0) => "has its channel to itself".to_string(),
        (same, 0) => format!("shares its channel with {same} other {}", plural(same)),
        (0, over) => format!(
            "is overlapped by {over} {} on nearby channels",
            plural(over)
        ),
        (same, over) => format!(
            "shares its channel with {same} other {} and is overlapped by {over} more",
            plural(same)
        ),
    };
    format!(
        "{} is on channel {} ({}) and {crowded}. Networks on the same channel take turns to \
         talk; networks on overlapping channels talk over each other, which is worse. {quietest}",
        name(network),
        channels::channel(network.freq_mhz),
        Band::of(network.freq_mhz).name()
    )
}

/// One band drawn as a chart: a lane per network with a bar over the slice of spectrum it fills,
/// the quietest channel marked, and the channel numbers along the bottom.
fn chart(sheet: &mut Sheet, band: Band, networks: &[&Network], quietest: u32) {
    let mut shown = networks.to_vec();
    shown.sort_by_key(|network| -network.signal_dbm);
    shown.truncate(MOST_LANES);
    // Back in channel order; the sort is stable, so each channel stays loudest first.
    shown.sort_by_key(|network| network.freq_mhz);
    let height = 26.0 + LANE * shown.len() as f32 + 24.0;
    sheet.heading(&format!("{} channels", band.name()), height);
    let (x, y) = (LEFT, sheet.y);
    let pdf = &mut sheet.pdf;

    // The band's edges: from its first channel to its last, or wider if a network reaches out.
    let labels = band.candidates();
    let edges = |channel: u32| channels::span(channels::frequency(band, channel), None, None);
    let spans = shown.iter().map(|network| network.span());
    let low = spans
        .clone()
        .map(|(low, _)| low)
        .fold(edges(labels[0]).0, u32::min);
    let last = *labels.last().expect("every band has candidates");
    let high = spans.map(|(_, high)| high).fold(edges(last).1, u32::max);
    let (axis_left, axis_right) = (x + NAMES, x + WIDTH);
    let at = |mhz: u32| {
        axis_left
            + (mhz.clamp(low, high) - low) as f32 / (high - low) as f32 * (axis_right - axis_left)
    };
    let (lanes_top, lanes_bottom) = (y + 22.0, y + 22.0 + LANE * shown.len() as f32);

    // 5 and 6 GHz have too many channels to label each; every other (or fourth) is enough to read
    // the axis by.
    let every = match band {
        Band::Ghz2 => 1,
        Band::Ghz5 => 2,
        Band::Ghz6 => 4,
    };
    for channel in labels.iter().step_by(every) {
        let centre = at(channels::frequency(band, *channel));
        pdf.line((centre, lanes_top), (centre, lanes_bottom), 0.5, BORDER);
        let label = channel.to_string();
        let left = centre - pdf::width(Font::Mono, 6.0, &label) / 2.0;
        pdf.text(left, lanes_bottom + 11.0, Font::Mono, 6.0, SUBTLE, &label);
    }
    let marker = at(channels::frequency(band, quietest));
    pdf.line((marker, lanes_top - 4.0), (marker, lanes_bottom), 1.2, FG);
    let label = format!("quietest · {quietest}");
    let left = marker - pdf::width(Font::MonoBold, 6.5, &label) / 2.0;
    pdf.text(left, y + 14.0, Font::MonoBold, 6.5, FG, &label);

    for (i, network) in shown.iter().enumerate() {
        let top = lanes_top + LANE * i as f32;
        let ink = if network.connected { AQUA } else { COOL };
        let font = if network.connected {
            Font::SansBold
        } else {
            Font::Sans
        };
        let label = format!(
            "{} · {}",
            name(network),
            channels::channel(network.freq_mhz)
        );
        let label = pdf::fit(font, 7.0, &label, NAMES - 24.0);
        pdf.text(
            x,
            top + 7.5,
            font,
            7.0,
            if network.connected { AQUA } else { FG },
            &label,
        );
        let (from, to) = network.span();
        pdf.bar(at(from), top + 2.5, at(to) - at(from), 5.0, ink);
    }
    let left_out = networks.len() - shown.len();
    if left_out > 0 {
        let note = format!("{left_out} quieter networks not drawn; the table lists them all");
        pdf.text(x, lanes_bottom + 20.0, Font::Sans, 6.5, SUBTLE, &note);
    }
    sheet.y += height + 4.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ssid: &str, freq_mhz: u32, signal_dbm: i32, connected: bool) -> Network {
        Network {
            ssid: Some(ssid.to_string()),
            bssid: Some("02:00:00:00:00:01".to_string()),
            freq_mhz,
            width_mhz: Some(20),
            centre_mhz: None,
            signal_dbm,
            connected,
        }
    }

    #[test]
    fn says_how_crowded_the_joined_channel_is() {
        let networks = [
            network("Home", 2437, -50, true),
            network("Neighbour", 2437, -70, false),
            network("Cafe", 2447, -75, false),
        ];
        let text = summary(&networks, Some(0), &[Band::Ghz2, Band::Ghz5]);
        assert!(text.starts_with(
            "Home is on channel 6 (2.4 GHz) and shares its channel with 1 other network and is \
             overlapped by 1 more."
        ));
        assert!(text.ends_with("The quietest channels here are 1 on 2.4 GHz and 36 on 5 GHz."));
    }

    #[test]
    fn writes_a_report_of_every_network() {
        let mut networks: Vec<Network> = (0..60)
            .map(|i| {
                network(
                    &format!("Net {i}"),
                    2412 + 5 * (i % 13),
                    -40 - i as i32,
                    i == 3,
                )
            })
            .collect();
        networks.push(network("Office", 5220, -60, false));
        let file = String::from_utf8_lossy(&document(&networks)).to_string();
        assert!(file.starts_with("%PDF"));
        assert!(file.contains("(61 networks heard)"));
        assert!(file.contains("(Net 59)"));
        assert!(file.contains("12 quieter networks not drawn"));
    }
}
