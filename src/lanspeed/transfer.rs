//! Moving the data and counting it: a sender writes for the test's seconds over every stream
//! at once, a receiver counts what arrives and reports it every interval. Both ends use the same
//! code, whichever way the data goes. The UDP receiver also counts the datagrams lost (by the
//! gaps in each stream's sequence) and the jitter (as RTP does: how much one datagram's transit
//! differs from the one before, smoothed).

use super::wire::{DATAGRAM, Datagram, Datagrams, Report, Stream};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a TCP sender writes at a time, and a receiver reads.
const CHUNK: usize = 128 * 1024;
/// How long after the sender's last second a receiver keeps listening for what is still on the
/// wire, before giving the stream up.
const GRACE: Duration = Duration::from_secs(2);
/// Sent after the last datagram, a few times, so a receiver learns the end even if one is lost.
const FINS: usize = 5;
/// How often a thread checks the clock while it waits.
const TICK: Duration = Duration::from_millis(10);
/// How long a UDP sender ahead of its pace waits before sending more: a short wait keeps the
/// bursts small enough for the receiver's buffer.
const PACE: Duration = Duration::from_millis(1);
/// What a UDP receiver's socket can hold while the reader is busy: the OS's default (a couple
/// of hundred kB) loses datagrams at a few hundred Mbps.
const RECEIVE_BUFFER: usize = 8 * 1024 * 1024;

/// Writes to every stream at once for `seconds`, then closes its side, reporting what it sent
/// every `interval` (the sending end's own count, for its screen) and handing back the whole
/// transfer's report.
pub fn send_tcp(
    streams: Vec<TcpStream>,
    seconds: Duration,
    interval: Duration,
    report: impl FnMut(&Report),
) -> Result<Report, String> {
    let chunk = vec![0u8; CHUNK];
    let start = Instant::now();
    let tally = Arc::new(Mutex::new(Tally::default()));
    // Senders still going; the clock ends once none is.
    let open = Arc::new(AtomicUsize::new(streams.len()));
    std::thread::scope(|scope| {
        let threads: Vec<_> = streams
            .into_iter()
            .map(|mut stream| {
                let (chunk, tally, open) = (&chunk, Arc::clone(&tally), Arc::clone(&open));
                scope.spawn(move || -> Result<(), String> {
                    let outcome = loop {
                        if start.elapsed() >= seconds {
                            break Ok(());
                        }
                        if let Err(e) = stream.write_all(chunk) {
                            break Err(e.to_string());
                        }
                        tally.lock().expect("a sender holds the tally").bytes += CHUNK as u64;
                    };
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    open.fetch_sub(1, Ordering::SeqCst);
                    outcome
                })
            })
            .collect();
        let total = clock(
            &tally,
            interval,
            |_| open.load(Ordering::SeqCst) == 0,
            report,
        );
        for thread in threads {
            thread.join().map_err(|_| "a sending thread died")??;
        }
        Ok(total)
    })
}

/// Reads every stream until the sender closes it, reporting what arrived every `interval`, and
/// hands back the whole transfer's report.
pub fn receive_tcp(
    streams: Vec<TcpStream>,
    interval: Duration,
    report: impl FnMut(&Report),
) -> Result<Report, String> {
    let tally = Arc::new(Mutex::new(Tally::default()));
    // Readers still going; the clock ends once none is.
    let open = Arc::new(AtomicUsize::new(streams.len()));
    std::thread::scope(|scope| {
        let readers: Vec<_> = streams
            .into_iter()
            .map(|mut stream| {
                let (tally, open) = (Arc::clone(&tally), Arc::clone(&open));
                scope.spawn(move || -> Result<(), String> {
                    let mut chunk = vec![0u8; CHUNK];
                    let outcome = loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => break Ok(()),
                            Ok(got) => {
                                tally.lock().expect("a reader holds the tally").bytes += got as u64;
                            }
                            Err(e) => break Err(e.to_string()),
                        }
                    };
                    open.fetch_sub(1, Ordering::SeqCst);
                    outcome
                })
            })
            .collect();
        let total = clock(
            &tally,
            interval,
            |_| open.load(Ordering::SeqCst) == 0,
            report,
        );
        let mut outcome = Ok(());
        for reader in readers {
            if let Err(why) = reader.join().map_err(|_| "a reading thread died")? {
                outcome = Err(why);
            }
        }
        outcome.map(|()| total)
    })
}

/// Sends datagrams from every socket to its address at once for `seconds`, paced to `mbps`
/// between them all (0: as fast as it can), then a few fins, reporting what it sent every
/// `interval` and handing back the whole transfer's report.
pub fn send_udp(
    sockets: Vec<(Arc<UdpSocket>, SocketAddr)>,
    seconds: Duration,
    mbps: f32,
    interval: Duration,
    report: impl FnMut(&Report),
) -> Result<Report, String> {
    let streams = sockets.len().max(1);
    // Each stream's share of the rate, in bytes a second; None is unpaced.
    let pace = (mbps > 0.0).then(|| mbps as f64 * 1e6 / 8.0 / streams as f64);
    let start = Instant::now();
    let tally = Arc::new(Mutex::new(Tally::default()));
    let open = Arc::new(AtomicUsize::new(sockets.len()));
    std::thread::scope(|scope| {
        let threads: Vec<_> = sockets
            .into_iter()
            .map(|(socket, to)| {
                let (tally, open) = (Arc::clone(&tally), Arc::clone(&open));
                scope.spawn(move || -> Result<(), String> {
                    let mut seq = 0u32;
                    let mut sent = 0u64;
                    let outcome = loop {
                        let elapsed = start.elapsed();
                        if elapsed >= seconds {
                            break Ok(());
                        }
                        // Behind the pace: send. Ahead of it: wait a tick.
                        let due =
                            pace.map_or(u64::MAX, |pace| (elapsed.as_secs_f64() * pace) as u64);
                        if sent >= due {
                            std::thread::sleep(PACE.min(seconds - elapsed));
                            continue;
                        }
                        let packet = Datagram::Packet {
                            seq,
                            sent_ns: elapsed.as_nanos() as u64,
                        };
                        if let Err(e) = socket.send_to(&packet.bytes(), to) {
                            break Err(e.to_string());
                        }
                        seq = seq.wrapping_add(1);
                        sent += DATAGRAM as u64;
                        tally.lock().expect("a sender holds the tally").bytes += DATAGRAM as u64;
                    };
                    for _ in 0..FINS {
                        let _ = socket.send_to(&Datagram::Fin.bytes(), to);
                        std::thread::sleep(TICK);
                    }
                    open.fetch_sub(1, Ordering::SeqCst);
                    outcome
                })
            })
            .collect();
        let total = clock(
            &tally,
            interval,
            |_| open.load(Ordering::SeqCst) == 0,
            report,
        );
        for thread in threads {
            thread.join().map_err(|_| "a sending thread died")??;
        }
        Ok(total)
    })
}

/// Reads datagrams on every socket, `streams` of them expected, until each stream has said fin
/// or `seconds` plus a grace period have passed, reporting every `interval`. Datagrams that
/// aren't a stream's packets (the hellos that set a stream up) count for nothing.
pub fn receive_udp(
    sockets: &[Arc<UdpSocket>],
    streams: usize,
    seconds: Duration,
    interval: Duration,
    report: impl FnMut(&Report),
) -> Result<Report, String> {
    let tally = Arc::new(Mutex::new(Tally::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    for socket in sockets {
        socket
            .set_read_timeout(Some(TICK * 10))
            .map_err(|e| e.to_string())?;
        // The OS may cap it lower (Linux at net.core.rmem_max); what it allows still helps.
        let _ = socket2::SockRef::from(socket.as_ref()).set_recv_buffer_size(RECEIVE_BUFFER);
    }
    std::thread::scope(|scope| {
        let readers: Vec<_> = sockets
            .iter()
            .enumerate()
            .map(|(index, socket)| {
                let (tally, stop) = (Arc::clone(&tally), Arc::clone(&stop));
                scope.spawn(move || {
                    let mut buffer = vec![0u8; DATAGRAM + 64];
                    while !stop.load(Ordering::Relaxed) {
                        let Ok((got, from)) = socket.recv_from(&mut buffer) else {
                            continue;
                        };
                        let mut tally = tally.lock().expect("a reader holds the tally");
                        match Datagram::read(&buffer[..got]) {
                            Some(Datagram::Packet { seq, sent_ns }) => {
                                tally.packet((index, from), got, seq, sent_ns, start.elapsed());
                            }
                            Some(Datagram::Fin) => tally.fin((index, from)),
                            _ => {}
                        }
                    }
                })
            })
            .collect();
        let deadline = start + seconds + GRACE;
        let total = clock(
            &tally,
            interval,
            |tally| tally.fins >= streams || Instant::now() >= deadline,
            report,
        );
        stop.store(true, Ordering::Relaxed);
        for reader in readers {
            reader.join().map_err(|_| "a reading thread died")?;
        }
        Ok(total)
    })
}

/// Reports the tally every `interval` until `finished` says so, and hands back the total.
fn clock(
    tally: &Mutex<Tally>,
    interval: Duration,
    finished: impl Fn(&Tally) -> bool,
    mut report: impl FnMut(&Report),
) -> Report {
    let start = Instant::now();
    let mut last = Tally::default();
    let mut from = 0.0;
    let mut next = start + interval;
    loop {
        std::thread::sleep(TICK);
        let now = Instant::now();
        let snapshot = tally.lock().expect("the clock reads the tally").clone();
        let finished = finished(&snapshot);
        if now >= next || finished {
            let to = now.duration_since(start).as_secs_f32();
            report(&snapshot.since(&last, from, to));
            last = snapshot;
            from = to;
            next += interval;
        }
        if finished {
            let to = now.duration_since(start).as_secs_f32();
            return last.since(&Tally::default(), 0.0, to);
        }
    }
}

/// What has arrived so far, over the whole transfer.
#[derive(Clone, Default)]
struct Tally {
    bytes: u64,
    /// UDP: each stream's count so far. Empty for TCP.
    streams: HashMap<Stream, StreamTally>,
    /// UDP: streams that have said fin.
    fins: usize,
}

#[derive(Clone, Default)]
struct StreamTally {
    got: u64,
    /// The highest sequence number seen, so the ones never seen count as lost.
    highest: Option<u32>,
    /// The last datagram's transit (arrival minus sending time), in ns; the clocks of the two
    /// ends differ, which doesn't matter: only the change from one to the next is used.
    transit_ns: Option<i128>,
    jitter_ns: f64,
    fin: bool,
}

impl Tally {
    fn packet(&mut self, stream: Stream, bytes: usize, seq: u32, sent_ns: u64, at: Duration) {
        self.bytes += bytes as u64;
        let stream = self.streams.entry(stream).or_default();
        stream.got += 1;
        stream.highest = Some(stream.highest.map_or(seq, |highest| highest.max(seq)));
        let transit = at.as_nanos() as i128 - sent_ns as i128;
        if let Some(before) = stream.transit_ns {
            let step = (transit - before).abs() as f64;
            stream.jitter_ns += (step - stream.jitter_ns) / 16.0;
        }
        stream.transit_ns = Some(transit);
    }

    fn fin(&mut self, stream: Stream) {
        let stream = self.streams.entry(stream).or_default();
        if !stream.fin {
            stream.fin = true;
            self.fins += 1;
        }
    }

    fn lost(&self) -> u64 {
        self.streams
            .values()
            .map(|stream| {
                let expected = stream.highest.map_or(0, |highest| highest as u64 + 1);
                expected.saturating_sub(stream.got)
            })
            .sum()
    }

    /// What arrived since `before`, as the report for `from` to `to` seconds.
    fn since(&self, before: &Tally, from: f32, to: f32) -> Report {
        let datagrams = (!self.streams.is_empty() || !before.streams.is_empty()).then(|| {
            let got: u64 = self.streams.values().map(|stream| stream.got).sum();
            let got_before: u64 = before.streams.values().map(|stream| stream.got).sum();
            let jitter_ns = match self.streams.len() {
                0 => 0.0,
                streams => self.streams.values().map(|s| s.jitter_ns).sum::<f64>() / streams as f64,
            };
            Datagrams {
                got: got - got_before,
                lost: self.lost().saturating_sub(before.lost()),
                jitter_ms: (jitter_ns / 1e6) as f32,
            }
        });
        Report {
            from,
            to,
            bytes: self.bytes - before.bytes,
            datagrams,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn stream(port: u16) -> Stream {
        (0, SocketAddr::from(([192, 168, 1, 20], port)))
    }

    #[test]
    fn counts_the_datagrams_lost_by_the_gaps_in_each_stream() {
        let mut tally = Tally::default();
        let at = Duration::from_millis(1);
        for seq in [0, 1, 3, 4] {
            tally.packet(stream(5000), DATAGRAM, seq, 0, at);
        }
        for seq in [0, 2] {
            tally.packet(stream(5001), DATAGRAM, seq, 0, at);
        }
        // Out of order isn't lost.
        tally.packet(stream(5000), DATAGRAM, 2, 0, at);
        assert_eq!(tally.lost(), 1);
        assert_eq!(tally.bytes, 7 * DATAGRAM as u64);
        let report = tally.since(&Tally::default(), 0.0, 1.0);
        assert_eq!(
            report.datagrams,
            Some(Datagrams {
                got: 7,
                lost: 1,
                jitter_ms: 0.0
            })
        );
        assert_eq!(report.mbps(), 7.0 * DATAGRAM as f32 * 8.0 / 1e6);
        tally.fin(stream(5000));
        tally.fin(stream(5000));
        assert_eq!(tally.fins, 1);
    }

    #[test]
    fn jitter_smooths_the_change_in_transit() {
        let mut tally = Tally::default();
        // Sent every ms, arriving every ms: no jitter.
        for seq in 0..4u32 {
            let ms = seq as u64 * 1_000_000;
            tally.packet(
                stream(5000),
                DATAGRAM,
                seq,
                ms,
                Duration::from_nanos(ms + 500_000),
            );
        }
        assert_eq!(tally.streams[&stream(5000)].jitter_ns, 0.0);
        // One arrives 16 ms late: the jitter steps up a sixteenth of that.
        tally.packet(
            stream(5000),
            DATAGRAM,
            4,
            4_000_000,
            Duration::from_nanos(4_000_000 + 500_000 + 16_000_000),
        );
        assert_eq!(tally.streams[&stream(5000)].jitter_ns, 1_000_000.0);
        let report = tally.since(&Tally::default(), 0.0, 1.0);
        assert_eq!(report.datagrams.map(|d| d.jitter_ms), Some(1.0));
    }

    #[test]
    fn an_interval_is_the_change_since_the_one_before() {
        let before = Tally {
            bytes: 100,
            ..Tally::default()
        };
        let mut now = before.clone();
        now.bytes = 350;
        let report = now.since(&before, 1.0, 2.0);
        assert_eq!(report.bytes, 250);
        assert_eq!(report.datagrams, None);
        assert_eq!(report.mbps(), 0.002);
    }

    #[test]
    fn tcp_arrives_whole_over_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let addr = listener.local_addr().expect("a port");
        let sender = std::thread::spawn(move || {
            let streams = (0..2)
                .map(|_| TcpStream::connect(addr).expect("connect"))
                .collect();
            let mut sent = Vec::new();
            let total = send_tcp(
                streams,
                Duration::from_millis(300),
                Duration::from_millis(100),
                |report| sent.push(report.bytes),
            )
            .expect("sent");
            // The sender counts its own intervals too, adding up to its total.
            assert_eq!(sent.iter().sum::<u64>(), total.bytes);
            total.bytes
        });
        let streams = (0..2)
            .map(|_| listener.accept().expect("accept").0)
            .collect();
        let mut intervals = Vec::new();
        let total = receive_tcp(streams, Duration::from_millis(100), |report| {
            intervals.push(report.clone())
        })
        .expect("received");
        let sent = sender.join().expect("the sender");
        assert_eq!(total.bytes, sent);
        assert_eq!(intervals.iter().map(|r| r.bytes).sum::<u64>(), sent);
        assert!(intervals.len() >= 3, "{} intervals", intervals.len());
        assert!(total.to >= 0.3);
    }

    #[test]
    fn udp_arrives_paced_and_ends_on_the_fins() {
        let receiver = Arc::new(UdpSocket::bind("127.0.0.1:0").expect("loopback"));
        let to = receiver.local_addr().expect("a port");
        let sockets: Vec<_> = (0..2)
            .map(|_| {
                (
                    Arc::new(UdpSocket::bind("127.0.0.1:0").expect("a socket")),
                    to,
                )
            })
            .collect();
        let sender = std::thread::spawn(move || {
            send_udp(
                sockets,
                Duration::from_millis(300),
                8.0,
                Duration::from_millis(100),
                |_| {},
            )
            .map(|total| total.bytes)
        });
        let started = Instant::now();
        let total = receive_udp(
            &[receiver],
            2,
            Duration::from_millis(300),
            Duration::from_millis(100),
            |_| {},
        )
        .expect("received");
        let sent = sender.join().expect("the sender").expect("sent");
        // 8 Mbps for 0.3 s is 300 kB, give or take the pacing's ticks.
        assert!((250_000..=320_000).contains(&sent), "{sent} bytes sent");
        let datagrams = total.datagrams.expect("udp counts datagrams");
        assert_eq!(total.bytes, sent);
        assert_eq!(datagrams.lost, 0);
        assert_eq!(datagrams.got, sent / DATAGRAM as u64);
        // The fins ended it well before the grace period would have.
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
