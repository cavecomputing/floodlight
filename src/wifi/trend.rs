//! Every access point heard across a run of scans, for walking around with `--live`: each one
//! stays once heard (marked gone while out of range, so rows don't come and go), with the
//! change in its signal since the scan before and the loudest it has been.

use super::channels::Band;
use super::survey::Network;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    /// dBm gained since the scan before; None the first time it's heard.
    pub change: Option<i32>,
    /// The strongest signal heard from it so far.
    pub peak: i32,
    /// Whether the latest scan heard it. Its network keeps the last signal heard.
    pub heard: bool,
}

/// Every access point heard since the screen opened, as last heard, with its reading.
#[derive(Default)]
pub struct Trend {
    seen: HashMap<String, (Network, Reading)>,
}

impl Trend {
    /// Takes in a new scan. One that isn't in it stays, marked gone, keeping its peak.
    pub fn record(&mut self, networks: &[Network]) {
        for (_, reading) in self.seen.values_mut() {
            reading.heard = false;
        }
        for network in networks {
            let signal = network.signal_dbm;
            let reading = match self.seen.get(&key(network)) {
                Some((last, reading)) => Reading {
                    change: Some(signal - last.signal_dbm),
                    peak: reading.peak.max(signal),
                    heard: true,
                },
                None => Reading {
                    change: None,
                    peak: signal,
                    heard: true,
                },
            };
            self.seen.insert(key(network), (network.clone(), reading));
        }
    }

    /// Everything heard so far in an order that only a newcomer changes: by band, channel and
    /// BSSID, never by signal.
    pub fn networks(&self) -> Vec<(Network, Reading)> {
        let mut networks: Vec<(Network, Reading)> = self.seen.values().cloned().collect();
        networks.sort_by_key(|(network, _)| {
            (Band::of(network.freq_mhz), network.freq_mhz, key(network))
        });
        networks
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
    fn keeps_every_access_point_in_a_steady_order() {
        let mut trend = Trend::default();
        trend.record(&[heard("aa:bb:cc:00:00:02", -70)]);
        trend.record(&[
            heard("aa:bb:cc:00:00:02", -62),
            heard("aa:bb:cc:00:00:01", -80),
        ]);
        let networks = trend.networks();
        assert_eq!(networks[0].0.bssid.as_deref(), Some("aa:bb:cc:00:00:01"));
        assert_eq!(networks[0].1.change, None);
        assert_eq!(
            networks[1].1,
            Reading {
                change: Some(8),
                peak: -62,
                heard: true
            }
        );

        trend.record(&[heard("aa:bb:cc:00:00:02", -75)]);
        let networks = trend.networks();
        assert_eq!(networks.len(), 2, "the one out of range stays");
        assert!(!networks[0].1.heard);
        assert_eq!(networks[0].0.signal_dbm, -80);
        assert_eq!(
            networks[1].1,
            Reading {
                change: Some(-13),
                peak: -62,
                heard: true
            }
        );
    }
}
