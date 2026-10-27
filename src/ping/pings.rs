//! The pings themselves: `count` of them (or until stopped), `wait` apart, on a thread of their
//! own, and what the replies add up to. The screen hears each `Ping` as it comes back.

use super::echo::Pinger;
use crate::speedtest::runs::jitter;
use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// How long a reply has to come back before the ping counts as lost.
pub const TIMEOUT: Duration = Duration::from_secs(2);

pub struct Ping {
    /// 1 for the first.
    pub seq: u32,
    /// When it went out, like `14:05:09`.
    pub time: String,
    /// The round trip in ms, or why nothing came back (`timed out`, `unreachable`).
    pub reply: Result<f32, String>,
}

/// Every ping so far.
#[derive(Default)]
pub struct Pings {
    pub pings: Vec<Ping>,
}

impl Pings {
    /// The round trips of the pings that came back, in order.
    pub fn round_trips(&self) -> Vec<f32> {
        self.pings
            .iter()
            .filter_map(|ping| ping.reply.as_ref().ok().copied())
            .collect()
    }

    pub fn lost(&self) -> usize {
        self.pings.iter().filter(|ping| ping.reply.is_err()).count()
    }

    /// The share lost, as a percentage.
    pub fn loss(&self) -> f32 {
        match self.pings.len() {
            0 => 0.0,
            sent => self.lost() as f32 * 100.0 / sent as f32,
        }
    }

    pub fn min(&self) -> Option<f32> {
        self.round_trips().into_iter().reduce(f32::min)
    }

    pub fn max(&self) -> Option<f32> {
        self.round_trips().into_iter().reduce(f32::max)
    }

    pub fn average(&self) -> Option<f32> {
        let trips = self.round_trips();
        (!trips.is_empty()).then(|| trips.iter().sum::<f32>() / trips.len() as f32)
    }

    /// How much one reply's round trip differs from the next, on average.
    pub fn jitter(&self) -> Option<f32> {
        jitter(&self.round_trips())
    }
}

/// Starts pinging `ip` on a thread of its own, `count` times (0: until the receiver is dropped),
/// `wait` between one reply (or timeout) and the next ping, and hands back the stream of pings.
/// Fails at once when this machine won't let it ping.
pub fn start(ip: Ipv4Addr, count: u32, wait: Duration) -> Result<Receiver<Ping>, String> {
    let pinger = Pinger::new()?;
    let (tx, rx) = channel();
    let last = if count == 0 { u32::MAX } else { count };
    std::thread::spawn(move || {
        for seq in 1..=last {
            if seq > 1 {
                std::thread::sleep(wait);
            }
            let time = chrono::Local::now().format("%H:%M:%S").to_string();
            let reply = pinger
                .echo(ip, seq as u16, TIMEOUT)
                .map(|took| took.as_secs_f32() * 1000.0);
            if tx.send(Ping { seq, time, reply }).is_err() {
                return;
            }
        }
    });
    Ok(rx)
}

/// Pings `count` times and hands back every ping once the last is in, for the outputs that print.
/// `each` hears about every ping as it comes back.
pub fn finished(
    ip: Ipv4Addr,
    count: u32,
    wait: Duration,
    mut each: impl FnMut(&Ping),
) -> Result<Pings, String> {
    let mut pings = Pings::default();
    for ping in start(ip, count, wait)? {
        each(&ping);
        pings.pings.push(ping);
    }
    Ok(pings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pings(replies: &[Option<f32>]) -> Pings {
        let pings = replies
            .iter()
            .enumerate()
            .map(|(i, reply)| Ping {
                seq: i as u32 + 1,
                time: "14:05:09".to_string(),
                reply: reply.ok_or_else(|| "timed out".to_string()),
            })
            .collect();
        Pings { pings }
    }

    #[test]
    fn adds_up_the_replies_and_the_lost() {
        let pings = pings(&[Some(10.0), None, Some(12.0), Some(11.0), None]);
        assert_eq!(pings.round_trips(), vec![10.0, 12.0, 11.0]);
        assert_eq!(pings.lost(), 2);
        assert_eq!(pings.loss(), 40.0);
        assert_eq!(pings.min(), Some(10.0));
        assert_eq!(pings.max(), Some(12.0));
        assert_eq!(pings.average(), Some(11.0));
        assert_eq!(pings.jitter(), Some(1.5));
    }

    #[test]
    fn nothing_back_is_all_lost_and_no_latency() {
        let pings = pings(&[None, None]);
        assert_eq!(pings.loss(), 100.0);
        assert_eq!(pings.average(), None);
        assert_eq!(Pings::default().loss(), 0.0);
    }
}
