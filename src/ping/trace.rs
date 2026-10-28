//! A traceroute: an echo every `wait` to each hop on the way to a host, each one's TTL set to run
//! out at that hop, so the router there answers. Each hop's answers are its `Pings`, so the hop
//! where loss starts shows as plainly as the host's own.

use super::echo::Pinger;
use super::pings::{Ping, Pings, TIMEOUT};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// How far out the trace looks for the host, as the OS's own traceroute does.
pub const MAX_HOPS: u8 = 30;

/// What the trace hears.
pub enum Event {
    /// One echo to one hop: the ping, its `host` the hop's place on the way from 0 (TTL 1), and
    /// the router that answered, or the host itself once the echo got there.
    Echo(Ping, Option<Ipv4Addr>),
    /// A router's name, looked up the first time its hop heard from it.
    Name(Ipv4Addr, String),
}

/// One hop on the way.
#[derive(Default)]
pub struct Hop {
    /// Every router heard at this hop, in the order first heard (more than one when the traffic
    /// is shared over several links), with its name when it has one.
    pub routers: Vec<(Ipv4Addr, Option<String>)>,
    pub pings: Pings,
}

/// Every hop heard so far on the way to `ip`.
pub struct Trace {
    pub ip: Ipv4Addr,
    pub hops: Vec<Hop>,
}

impl Trace {
    pub fn new(ip: Ipv4Addr) -> Trace {
        Trace {
            ip,
            hops: Vec::new(),
        }
    }

    pub fn add(&mut self, event: Event) {
        let (ping, from) = match event {
            Event::Echo(ping, from) => (ping, from),
            Event::Name(ip, name) => {
                for hop in &mut self.hops {
                    for (router, named) in &mut hop.routers {
                        if *router == ip {
                            *named = Some(name.clone());
                        }
                    }
                }
                return;
            }
        };
        let at = ping.host;
        if self.hops.len() <= at {
            self.hops.resize_with(at + 1, Hop::default);
        }
        let hop = &mut self.hops[at];
        if let Some(from) = from
            && !hop.routers.iter().any(|(router, _)| *router == from)
        {
            hop.routers.push((from, None));
        }
        hop.pings.pings.push(ping);
    }

    /// Whether the echoes got all the way to the host.
    pub fn reached(&self) -> bool {
        self.hops.iter().any(|hop| self.is_host(hop))
    }

    /// The hops on the way: up to the first the host itself answered at (the ones past it heard
    /// from the host too), or while it hasn't, up to the last that anything answered at.
    pub fn path(&self) -> &[Hop] {
        let end = match self.hops.iter().position(|hop| self.is_host(hop)) {
            Some(at) => at + 1,
            None => self
                .hops
                .iter()
                .rposition(|hop| !hop.routers.is_empty())
                .map_or(0, |at| at + 1),
        };
        &self.hops[..end]
    }

    /// The hop number (from 1) where the loss that reaches the host starts: the first of the last
    /// run of hops that all lost some. A router that loses echoes when the hops after it don't is
    /// only slow to answer for itself, which routers are allowed to be, so it doesn't count.
    pub fn loss_starts(&self) -> Option<usize> {
        let path = self.path();
        if !self.reached() || path.last()?.pings.lost() == 0 {
            return None;
        }
        let clean = path.iter().rposition(|hop| hop.pings.lost() == 0);
        Some(clean.map_or(1, |at| at + 2))
    }

    /// How many rounds have gone out so far.
    pub fn rounds(&self) -> u32 {
        let last = |hop: &Hop| hop.pings.pings.last().map_or(0, |ping| ping.seq);
        self.hops.iter().map(last).max().unwrap_or(0)
    }

    fn is_host(&self, hop: &Hop) -> bool {
        hop.routers.iter().any(|(router, _)| *router == self.ip)
    }
}

/// Starts an echo to each hop on the way to `ip` on a thread of its own, `count` rounds of them
/// (0: until the receiver is dropped), `wait` between one answer (or timeout) and the next, and
/// hands back the stream of them all. The hops past the one the host answered at stop. Fails at
/// once when this machine won't let it ping.
pub fn start(ip: Ipv4Addr, count: u32, wait: Duration) -> Result<Receiver<Event>, String> {
    let (tx, rx) = channel();
    let last = if count == 0 { u32::MAX } else { count };
    // The lowest TTL the host itself has answered at so far.
    let host_at = Arc::new(AtomicU8::new(u8::MAX));
    for ttl in 1..=MAX_HOPS {
        let mut pinger = Pinger::new()?;
        pinger.set_ttl(ttl)?;
        let (tx, host_at) = (tx.clone(), host_at.clone());
        std::thread::spawn(move || {
            let mut heard = Vec::new();
            for seq in 1..=last {
                if seq > 1 {
                    std::thread::sleep(wait);
                }
                if ttl > host_at.load(Ordering::Relaxed) {
                    return;
                }
                let time = chrono::Local::now().format("%H:%M:%S").to_string();
                let answer = pinger.echo(ip, seq as u16, TIMEOUT);
                let from = answer.as_ref().ok().map(|answer| answer.from);
                if from == Some(ip) {
                    host_at.fetch_min(ttl, Ordering::Relaxed);
                }
                // A router new to this hop is looked up on a thread of its own, since a name
                // server can take seconds to answer.
                if let Some(router) = from
                    && !heard.contains(&router)
                {
                    heard.push(router);
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        if let Ok(name) = dns_lookup::lookup_addr(&IpAddr::V4(router))
                            && name != router.to_string()
                        {
                            let _ = tx.send(Event::Name(router, name));
                        }
                    });
                }
                let ping = Ping {
                    host: usize::from(ttl - 1),
                    seq,
                    time,
                    reply: answer.map(|answer| answer.took.as_secs_f32() * 1000.0),
                };
                if tx.send(Event::Echo(ping, from)).is_err() {
                    return;
                }
            }
        });
    }
    Ok(rx)
}

/// Traces `count` rounds and hands back what they found, for the outputs that print.
pub fn finished(ip: Ipv4Addr, count: u32, wait: Duration) -> Result<Trace, String> {
    let mut trace = Trace::new(ip);
    for event in start(ip, count, wait)? {
        trace.add(event);
    }
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 20);

    /// A trace whose hops answered as given, a round a reply: a router's last byte, or lost.
    fn traced(hops: &[&[Option<u8>]]) -> Trace {
        let mut trace = Trace::new(HOST);
        for (at, replies) in hops.iter().enumerate() {
            for (round, reply) in replies.iter().enumerate() {
                let from = reply.map(|last| Ipv4Addr::new(192, 168, 1, last));
                let ping = Ping {
                    host: at,
                    seq: round as u32 + 1,
                    time: "14:05:09".to_string(),
                    reply: match from {
                        Some(_) => Ok(at as f32 + 1.0),
                        None => Err("timed out".to_string()),
                    },
                };
                trace.add(Event::Echo(ping, from));
            }
        }
        trace
    }

    #[test]
    fn the_path_ends_where_the_host_answered() {
        // The fourth hop heard from the host too, so it isn't on the way.
        let trace = traced(&[&[Some(1)], &[None], &[Some(20)], &[Some(20)]]);
        assert!(trace.reached());
        assert_eq!(trace.path().len(), 3);
        assert_eq!(trace.rounds(), 1);
        // Not there yet: up to the last hop that answered, the silent ones after it left off.
        let trace = traced(&[&[Some(1)], &[Some(2)], &[None]]);
        assert!(!trace.reached());
        assert_eq!(trace.path().len(), 2);
    }

    #[test]
    fn keeps_each_router_heard_at_a_hop_once() {
        let trace = traced(&[&[Some(1), Some(2), Some(1), None]]);
        let routers: Vec<Ipv4Addr> = trace.hops[0].routers.iter().map(|r| r.0).collect();
        assert_eq!(
            routers,
            [Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(192, 168, 1, 2)]
        );
        assert_eq!(trace.hops[0].pings.lost(), 1);
    }

    #[test]
    fn loss_starts_where_it_carries_on_to_the_host() {
        let lossy = [Some(20), None, Some(20)];
        // Hop 2 loses some for itself only; from hop 3 on, it carries on to the host.
        let trace = traced(&[
            &[Some(1), Some(1), Some(1)],
            &[Some(2), None, Some(2)],
            &[Some(3), Some(3), Some(3)],
            &[Some(4), None, Some(4)],
            &lossy,
        ]);
        assert_eq!(trace.loss_starts(), Some(4));
        // Nothing lost at the host: no loss starts anywhere, whatever the routers lost.
        let trace = traced(&[&[Some(1), None], &[Some(20), Some(20)]]);
        assert_eq!(trace.loss_starts(), None);
        // Lost from the first hop on.
        let trace = traced(&[&[Some(1), None], &lossy]);
        assert_eq!(trace.loss_starts(), Some(1));
    }
}
