//! How each access point's signal moves from scan to scan, for walking around with `--live`:
//! the change since the scan before and the loudest it has been.

use super::survey::Network;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    /// dBm gained since the scan before; None the first time it's heard.
    pub change: Option<i32>,
    /// The strongest signal heard from it so far.
    pub peak: i32,
}

/// The latest reading of every access point heard since the screen opened, with its last signal.
#[derive(Default)]
pub struct Trend {
    seen: HashMap<String, (i32, Reading)>,
}

impl Trend {
    /// Takes in a new scan. An access point that drops out keeps its peak for when it's back.
    pub fn record(&mut self, networks: &[Network]) {
        for network in networks {
            let signal = network.signal_dbm;
            let reading = match self.seen.get(&key(network)) {
                Some((last, reading)) => Reading {
                    change: Some(signal - last),
                    peak: reading.peak.max(signal),
                },
                None => Reading {
                    change: None,
                    peak: signal,
                },
            };
            self.seen.insert(key(network), (signal, reading));
        }
    }

    pub fn of(&self, network: &Network) -> Option<Reading> {
        self.seen.get(&key(network)).map(|(_, reading)| *reading)
    }
}

/// One access point: its BSSID, or its name and channel where the OS gives no BSSID.
pub fn key(network: &Network) -> String {
    match &network.bssid {
        Some(bssid) => bssid.clone(),
        None => format!("{:?}@{}", network.ssid, network.freq_mhz),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(bssid: &str, signal_dbm: i32) -> Network {
        Network {
            ssid: Some("test".to_string()),
            bssid: Some(bssid.to_string()),
            freq_mhz: 2437,
            width_mhz: None,
            signal_dbm,
            connected: false,
        }
    }

    #[test]
    fn follows_each_access_point() {
        let mut trend = Trend::default();
        trend.record(&[heard("aa:bb:cc:00:00:01", -70)]);
        let now = [
            heard("aa:bb:cc:00:00:01", -62),
            heard("aa:bb:cc:00:00:02", -80),
        ];
        trend.record(&now);
        assert_eq!(
            trend.of(&now[0]),
            Some(Reading {
                change: Some(8),
                peak: -62
            })
        );
        assert_eq!(trend.of(&now[1]).unwrap().change, None);

        let now = [heard("aa:bb:cc:00:00:01", -75)];
        trend.record(&now);
        assert_eq!(
            trend.of(&now[0]),
            Some(Reading {
                change: Some(-13),
                peak: -62
            })
        );
    }
}
