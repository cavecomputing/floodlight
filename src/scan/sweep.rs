//! The sweep itself: every address probed at once (up to a limit), the ARP table read as replies
//! come in, makers and names filled in. The screen hears about it through `Event`s.

use super::arp;
use super::hosts::{Hosts, Vendor};
use super::mdns;
use super::ping;
use super::vendor::Vendors;
use ipnet::Ipv4Net;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};

/// Pings in flight at once. A /24 takes a couple of rounds.
const AT_ONCE: usize = 128;
/// How long a host gets to answer a ping.
const REPLY_WAIT: Duration = Duration::from_millis(700);
/// How often the ARP table is read while pings are out.
const ARP_EVERY: Duration = Duration::from_millis(400);
/// How long a name lookup gets.
const NAME_WAIT: Duration = Duration::from_secs(2);

pub enum Event {
    /// One address was pinged, with its round trip if it answered.
    Probed {
        ip: Ipv4Addr,
        latency_ms: Option<f32>,
    },
    /// The ARP table has a MAC for the address.
    Mac {
        ip: Ipv4Addr,
        mac: String,
        vendor: Vendor,
    },
    Name {
        ip: Ipv4Addr,
        name: String,
    },
    Done {
        took: Duration,
    },
}

/// Starts a sweep on the runtime and hands back the stream of what it finds. Dropping the
/// receiver ends the sweep.
pub fn start(runtime: &Runtime, net: Ipv4Net) -> UnboundedReceiver<Event> {
    let (tx, rx) = unbounded_channel();
    runtime.spawn(run(net, tx));
    rx
}

/// Runs a whole sweep and hands back the table once the last answer is in, for the outputs that
/// print rather than show it.
pub fn finished(runtime: &Runtime, net: Ipv4Net) -> Hosts {
    let mut events = start(runtime, net);
    let mut hosts = Hosts::new(net.hosts().count());
    runtime.block_on(async {
        while let Some(event) = events.recv().await {
            let done = matches!(event, Event::Done { .. });
            hosts.apply(event);
            if done {
                break;
            }
        }
    });
    hosts
}

async fn run(net: Ipv4Net, tx: UnboundedSender<Event>) {
    let started = Instant::now();
    let probes_done = Arc::new(AtomicBool::new(false));
    tokio::spawn(probe_all(net, tx.clone(), probes_done.clone()));
    let vendors = Vendors::load();

    let mut known: HashMap<Ipv4Addr, String> = HashMap::new();
    let mut names = JoinSet::new();
    loop {
        sleep(ARP_EVERY).await;
        if tx.is_closed() {
            return;
        }
        let finishing = probes_done.load(Ordering::Relaxed);
        if finishing {
            // The last replies' ARP entries land a moment after the pings.
            sleep(ARP_EVERY).await;
        }
        for (ip, mac) in arp::read().await {
            if !net.contains(&ip) || known.get(&ip) == Some(&mac) {
                continue;
            }
            let vendor = vendors.lookup(&mac);
            let _ = tx.send(Event::Mac {
                ip,
                mac: mac.clone(),
                vendor,
            });
            if known.insert(ip, mac).is_none() {
                names.spawn(name(ip, tx.clone()));
            }
        }
        if finishing {
            break;
        }
    }
    // The hosts found last are still being asked their names; the sweep isn't done, and the
    // printed outputs can't stop reading, until they have answered or given up.
    while names.join_next().await.is_some() {}
    let _ = tx.send(Event::Done {
        took: started.elapsed(),
    });
}

/// Pings every address, `AT_ONCE` at a time, and raises the flag when the last one is back.
async fn probe_all(net: Ipv4Net, tx: UnboundedSender<Event>, done: Arc<AtomicBool>) {
    let slots = Arc::new(Semaphore::new(AT_ONCE));
    let mut probes = JoinSet::new();
    for ip in net.hosts() {
        let slots = slots.clone();
        let tx = tx.clone();
        probes.spawn(async move {
            let _slot = slots
                .acquire_owned()
                .await
                .expect("the semaphore is never closed");
            if tx.is_closed() {
                return;
            }
            let latency_ms = ping::probe(ip, REPLY_WAIT).await;
            let _ = tx.send(Event::Probed { ip, latency_ms });
        });
    }
    while probes.join_next().await.is_some() {}
    done.store(true, Ordering::Relaxed);
}

/// Asks the host itself over mDNS and the DNS server at the same time; the host's own name wins.
async fn name(ip: Ipv4Addr, tx: UnboundedSender<Event>) {
    let (own, listed) = tokio::join!(mdns::name(ip, NAME_WAIT), dns_name(ip));
    if let Some(name) = own.or(listed) {
        let _ = tx.send(Event::Name { ip, name });
    }
}

/// Reverse DNS on a thread of its own (the OS call blocks), given `NAME_WAIT`.
async fn dns_name(ip: Ipv4Addr) -> Option<String> {
    let lookup = tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&IpAddr::V4(ip)));
    let found = timeout(NAME_WAIT, lookup).await.ok()?.ok()?.ok()?;
    let found = found.trim_end_matches('.').to_string();
    (!found.is_empty() && found != ip.to_string()).then_some(found)
}
