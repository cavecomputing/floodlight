//! WiFi channels as frequencies: which band a network is on, which slice of it the network
//! fills, which neighbours share or overlap that slice, and which channel is quietest.

use super::survey::Network;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    Ghz2,
    Ghz5,
    Ghz6,
}

impl Band {
    pub fn of(freq_mhz: u32) -> Band {
        match freq_mhz {
            ..5000 => Band::Ghz2,
            5000..5925 => Band::Ghz5,
            _ => Band::Ghz6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Band::Ghz2 => "2.4 GHz",
            Band::Ghz5 => "5 GHz",
            Band::Ghz6 => "6 GHz",
        }
    }

    /// The centre of channel 0, from which every channel is 5 MHz apart (14 is the odd one out).
    fn base_mhz(self) -> u32 {
        match self {
            Band::Ghz2 => 2407,
            Band::Ghz5 => 5000,
            Band::Ghz6 => 5950,
        }
    }

    /// The centres the overlap strip has a cell for: every 2.4 GHz channel, every 20 MHz
    /// channel at 5 GHz, and every 40 MHz block at 6 GHz (its 20 MHz channels would not fit).
    fn cells(self) -> Vec<u32> {
        match self {
            Band::Ghz2 => (1..=13).map(|channel| frequency(self, channel)).collect(),
            Band::Ghz5 => twenty_mhz_5ghz()
                .map(|channel| frequency(self, channel))
                .collect(),
            Band::Ghz6 => (0..30).map(|block| 5965 + 40 * block).collect(),
        }
    }

    /// Where a new access point could go: the three 2.4 GHz channels that don't overlap each
    /// other, and every 20 MHz channel at 5 and 6 GHz.
    pub fn candidates(self) -> Vec<u32> {
        match self {
            Band::Ghz2 => vec![1, 6, 11],
            Band::Ghz5 => twenty_mhz_5ghz().collect(),
            Band::Ghz6 => (1..=233).step_by(4).collect(),
        }
    }
}

/// The 20 MHz channels at 5 GHz, in their three runs.
fn twenty_mhz_5ghz() -> impl Iterator<Item = u32> {
    [(36, 64), (100, 144), (149, 177)]
        .into_iter()
        .flat_map(|(first, last)| (first..=last).step_by(4))
}

/// The centre frequency of a channel number in a band.
pub fn frequency(band: Band, channel: u32) -> u32 {
    if band == Band::Ghz2 && channel == 14 {
        return 2484;
    }
    band.base_mhz() + 5 * channel
}

/// The channel number at a centre frequency.
pub fn channel(freq_mhz: u32) -> u32 {
    if freq_mhz == 2484 {
        return 14;
    }
    let band = Band::of(freq_mhz);
    freq_mhz.saturating_sub(band.base_mhz()) / 5
}

/// The slice of spectrum a network fills, low and high edge in MHz. A width the OS didn't report
/// counts as 20 MHz. A 2.4 GHz channel is 22 MHz wide (the classic mask that makes only 1, 6
/// and 11 clear of each other). Where the beacon said where the bonded channel is centred
/// (`centre_mhz`), the slice sits around that. Otherwise it is guessed: at 2.4 GHz the 40 MHz
/// mode reaches up from the low channels and down from the high ones, and wider 5 and 6 GHz
/// channels sit in fixed blocks (36-48 is one 80 MHz block, so a network on 44 at 80 MHz fills
/// 36 to 48), 320 MHz ones in the blocks starting at channel 1 (there is a second layout 160 MHz
/// up, which only the beacon can tell).
pub fn span(freq_mhz: u32, width_mhz: Option<u32>, centre_mhz: Option<u32>) -> (u32, u32) {
    let width = width_mhz.unwrap_or(20).max(20);
    let band = Band::of(freq_mhz);
    // The extra 2 MHz of the 22 MHz mask.
    let skirt = if band == Band::Ghz2 { 1 } else { 0 };
    if let Some(centre) = centre_mhz {
        return (centre - width / 2 - skirt, centre + width / 2 + skirt);
    }
    match band {
        Band::Ghz2 if width >= 40 && channel(freq_mhz) <= 7 => (freq_mhz - 11, freq_mhz + 31),
        Band::Ghz2 if width >= 40 => (freq_mhz - 31, freq_mhz + 11),
        Band::Ghz2 => (freq_mhz - 11, freq_mhz + 11),
        band => {
            // The 5 GHz blocks restart at channel 149, which isn't on the grid of the ones below.
            let start = match band {
                Band::Ghz5 if freq_mhz >= 5735 => 5735,
                Band::Ghz5 => 5170,
                _ => 5945,
            };
            let low = start + (freq_mhz.saturating_sub(start + 10) / width) * width;
            (low, low + width)
        }
    }
}

/// The networks near one network: on its primary channel (they take turns, so they share
/// airtime), or on another one that overlaps its slice (they talk over each other, which is
/// worse).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Crowd {
    pub same: usize,
    pub overlapping: usize,
}

/// The crowd around each network, in the same order.
pub fn crowds(networks: &[Network]) -> Vec<Crowd> {
    networks
        .iter()
        .enumerate()
        .map(|(i, network)| {
            let (low, high) = network.span();
            let mut crowd = Crowd::default();
            for (j, other) in networks.iter().enumerate() {
                let (other_low, other_high) = other.span();
                if i == j || other_low >= high || other_high <= low {
                    continue;
                }
                if other.freq_mhz == network.freq_mhz {
                    crowd.same += 1;
                } else {
                    crowd.overlapping += 1;
                }
            }
            crowd
        })
        .collect()
}

/// The channel in a band with the least heard on it, every network that covers it weighed by
/// how loud it is (a network at -90 dBm or below barely counts). The lowest channel wins a tie.
pub fn quietest(networks: &[Network], band: Band) -> u32 {
    let load = |channel: u32| -> i32 {
        let centre = frequency(band, channel);
        networks
            .iter()
            .filter(|network| {
                let (low, high) = network.span();
                low < centre && centre < high
            })
            .map(|network| (network.signal_dbm + 100).max(1))
            .sum()
    };
    band.candidates()
        .into_iter()
        .min_by_key(|channel| load(*channel))
        .expect("every band has candidates")
}

/// The band drawn as a row of cells, `─` where the network fills it and `●` on its primary
/// channel, so the rows of one band stacked together show who overlaps whom.
pub fn strip(freq_mhz: u32, width_mhz: Option<u32>, centre_mhz: Option<u32>) -> String {
    let (low, high) = span(freq_mhz, width_mhz, centre_mhz);
    let cells = Band::of(freq_mhz).cells();
    let primary = cells
        .iter()
        .enumerate()
        .min_by_key(|(_, centre)| centre.abs_diff(freq_mhz))
        .map(|(i, _)| i);
    cells
        .iter()
        .enumerate()
        .map(|(i, centre)| match i {
            _ if Some(i) == primary => '●',
            _ if low <= *centre && *centre <= high => '─',
            _ => '·',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(channel: u32, band: Band, width_mhz: Option<u32>, signal_dbm: i32) -> Network {
        Network {
            ssid: Some("test".to_string()),
            bssid: None,
            freq_mhz: frequency(band, channel),
            width_mhz,
            centre_mhz: None,
            signal_dbm,
            connected: false,
        }
    }

    #[test]
    fn channels_and_frequencies_round_trip() {
        for (band, number, freq) in [
            (Band::Ghz2, 1, 2412),
            (Band::Ghz2, 14, 2484),
            (Band::Ghz5, 36, 5180),
            (Band::Ghz5, 149, 5745),
            (Band::Ghz6, 1, 5955),
        ] {
            assert_eq!(frequency(band, number), freq);
            assert_eq!(Band::of(freq), band);
            assert_eq!(channel(freq), number);
        }
    }

    #[test]
    fn only_one_six_and_eleven_stay_apart() {
        let one = span(2412, None, None);
        let five = span(2432, None, None);
        let six = span(2437, None, None);
        let eleven = span(2462, None, None);
        assert!(one.1 > five.0, "1 and 5 overlap");
        assert!(one.1 <= six.0 && six.1 <= eleven.0);
    }

    #[test]
    fn wide_channels_fill_their_block() {
        assert_eq!(span(5220, Some(80), None), (5170, 5250)); // 44 at 80 MHz: 36 to 48
        assert_eq!(span(5745, Some(80), None), (5735, 5815)); // 149 at 80 MHz: 149 to 161
        assert_eq!(span(5320, Some(160), None), (5170, 5330)); // 64 at 160 MHz: 36 to 64
        assert_eq!(span(5200, Some(40), None), (5170, 5210)); // 40 at 40 MHz: 36 and 40
        assert_eq!(span(2412, Some(40), None), (2401, 2443)); // 1 reaches up
        assert_eq!(span(2462, Some(40), None), (2431, 2473)); // 11 reaches down
        assert_eq!(span(5955, Some(320), None), (5945, 6265)); // 6 GHz 1 at 320 MHz: 1 to 61
    }

    #[test]
    fn a_centre_from_the_beacon_beats_the_guess() {
        // 7 at 40 MHz paired with 3 below, where the guess would reach up to 11.
        assert_eq!(span(2442, Some(40), Some(2432)), (2411, 2453));
        assert_eq!(span(5180, Some(80), Some(5210)), (5170, 5250));
        // 6 GHz 65 at 320 MHz in the second layout (centre 95), not the block from channel 1.
        assert_eq!(span(6275, Some(320), Some(6425)), (6265, 6585));
        assert_eq!(
            strip(6275, Some(320), Some(6425)),
            "········●───────··············"
        );
    }

    #[test]
    fn crowds_tell_sharing_from_overlapping() {
        let networks = [
            network(6, Band::Ghz2, None, -50),
            network(6, Band::Ghz2, None, -70),
            network(8, Band::Ghz2, None, -60),
            network(1, Band::Ghz2, None, -60),
            network(36, Band::Ghz5, Some(80), -60),
            network(44, Band::Ghz5, None, -60),
        ];
        let crowds = crowds(&networks);
        assert_eq!(
            crowds[0],
            Crowd {
                same: 1,
                overlapping: 1
            }
        );
        assert_eq!(crowds[3], Crowd::default());
        assert_eq!(crowds[4].overlapping, 1);
    }

    #[test]
    fn quietest_avoids_the_loud_channel() {
        let networks = [
            network(1, Band::Ghz2, None, -40),
            network(11, Band::Ghz2, None, -80),
        ];
        assert_eq!(quietest(&networks, Band::Ghz2), 6);
        assert_eq!(quietest(&[], Band::Ghz5), 36);
    }

    #[test]
    fn strips_show_the_slice_and_the_primary() {
        assert_eq!(strip(2437, None, None), "···──●──·····");
        assert_eq!(strip(2412, None, None), "●──··········");
        assert_eq!(strip(5220, Some(80), None), "──●─························");
    }
}
