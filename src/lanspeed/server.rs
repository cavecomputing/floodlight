//! The serving end: listens on one port, TCP and UDP both, and answers each client's hello with
//! a test. It takes one test at a time (a second client is told `busy`), receives or sends as
//! asked, and tells its screen (or the plain printer) what happened through `Event`s, kept in
//! `Tests` the way the client's `Runs` are. It runs until you quit.

use super::runs::Lines;
use super::transfer;
use super::wire::{self, DATAGRAM, Datagram, Direction, Protocol, Report, Test};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a client has to open its streams after its hello.
const SETUP: Duration = Duration::from_secs(5);

/// Tests waiting for their streams, by token: each hands its TCP streams on through a channel.
type Pending = Arc<Mutex<HashMap<String, Sender<TcpStream>>>>;

pub enum Event {
    /// A client's test began: when, from where, and what it asked for.
    Started {
        time: String,
        from: IpAddr,
        test: Test,
    },
    /// What moved over one interval of the test under way: received from the client, or sent
    /// to it (this end's own count).
    Interval(Report),
    /// The test is over: the whole transfer's report, or why it failed.
    Finished(Report),
    Failed(String),
}

/// One client's test, filled in as it goes.
pub struct Served {
    pub time: String,
    pub from: IpAddr,
    pub test: Test,
    pub intervals: Vec<Report>,
    pub total: Option<Report>,
    pub failed: Option<String>,
}

impl Served {
    /// Still moving data: neither finished nor failed.
    pub fn under_way(&self) -> bool {
        self.total.is_none() && self.failed.is_none()
    }

    /// `received` for a client's upload, `sent` for its download: what this end did.
    pub fn did(&self) -> &'static str {
        match self.test.direction {
            Direction::Upload => "received",
            Direction::Download => "sent",
        }
    }
}

/// Every test served so far.
#[derive(Default)]
pub struct Tests {
    pub tests: Vec<Served>,
}

impl Tests {
    pub fn apply(&mut self, event: Event) {
        if let Event::Started { time, from, test } = event {
            self.tests.push(Served {
                time,
                from,
                test,
                intervals: Vec::new(),
                total: None,
                failed: None,
            });
            return;
        }
        let Some(served) = self.tests.last_mut() else {
            return;
        };
        match event {
            Event::Interval(report) => served.intervals.push(report),
            Event::Finished(report) => served.total = Some(report),
            Event::Failed(why) => served.failed = Some(why),
            Event::Started { .. } => {}
        }
    }

    /// Every interval so far of the tests one way, across the clients, for the graphs.
    pub fn intervals(&self, direction: Direction) -> Vec<Option<f32>> {
        self.tests
            .iter()
            .filter(|served| served.test.direction == direction)
            .flat_map(|served| served.intervals.iter())
            .map(|report| Some(report.mbps()))
            .collect()
    }
}

/// Listens on `port`, TCP and UDP both, and serves tests on threads of their own until the
/// program ends, handing back the stream of what happens. Fails only when it can't listen.
pub fn serve(port: u16) -> anyhow::Result<Receiver<Event>> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| anyhow::anyhow!("can't listen on tcp port {port}: {e}"))?;
    let udp = Arc::new(
        UdpSocket::bind(("0.0.0.0", port))
            .map_err(|e| anyhow::anyhow!("can't listen on udp port {port}: {e}"))?,
    );
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let pending: Pending = Arc::default();
        let busy = Arc::new(AtomicBool::new(false));
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let (pending, busy, udp) = (Arc::clone(&pending), Arc::clone(&busy), Arc::clone(&udp));
            let tx = tx.clone();
            std::thread::spawn(move || answer(stream, &pending, &busy, &udp, &tx));
        }
    });
    Ok(rx)
}

/// One connection: a hello starts a test; a data line joins one.
fn answer(
    stream: TcpStream,
    pending: &Pending,
    busy: &AtomicBool,
    udp: &Arc<UdpSocket>,
    tx: &Sender<Event>,
) {
    let Ok(from) = stream.peer_addr() else {
        return;
    };
    let _ = stream.set_read_timeout(Some(SETUP));
    let mut control = Lines::new(stream);
    let Ok(line) = control.hear() else {
        return;
    };
    let (word, fields) = wire::fields(&line);
    if word == wire::DATA {
        let token = fields
            .iter()
            .find(|(key, _)| *key == "token")
            .map(|(_, token)| *token)
            .unwrap_or_default();
        if let Some(streams) = pending.lock().expect("pending tests").get(token) {
            let _ = streams.send(control.into_inner());
        }
        return;
    }
    let test = match Test::from_hello(&line) {
        Ok(test) => test,
        Err(why) => {
            let _ = control.say(&format!("{} {why}", wire::REFUSED));
            return;
        }
    };
    if busy.swap(true, Ordering::SeqCst) {
        let _ = control.say(wire::BUSY);
        return;
    }
    let token = format!("{:x}", Instant::now().elapsed().as_nanos() ^ rand());
    let (streams, rx) = channel();
    pending
        .lock()
        .expect("pending tests")
        .insert(token.clone(), streams);
    let _ = tx.send(Event::Started {
        time: chrono::Local::now().format("%H:%M:%S").to_string(),
        from: from.ip(),
        test: test.clone(),
    });
    let outcome = control
        .say(&format!("{} token={token}", wire::OK))
        .and_then(|()| run(&test, &token, &mut control, rx, udp, tx));
    pending.lock().expect("pending tests").remove(&token);
    busy.store(false, Ordering::SeqCst);
    let _ = tx.send(match outcome {
        Ok(total) => Event::Finished(total),
        Err(why) => {
            let _ = control.say(&format!("{} {why}", wire::REFUSED));
            Event::Failed(why)
        }
    });
}

/// Serves one test and hands back the whole transfer's report: what was received (reported to
/// the client as it went) or what was sent.
fn run(
    test: &Test,
    token: &str,
    control: &mut Lines,
    streams: Receiver<TcpStream>,
    udp: &Arc<UdpSocket>,
    tx: &Sender<Event>,
) -> Result<Report, String> {
    let interval = |report: &Report| {
        let _ = tx.send(Event::Interval(report.clone()));
    };
    let total = match test.protocol {
        Protocol::Tcp => {
            let mut tcp = Vec::new();
            for _ in 0..test.streams {
                let stream = streams
                    .recv_timeout(SETUP)
                    .map_err(|_| "the client didn't open its streams in time".to_string())?;
                tcp.push(stream);
            }
            match test.direction {
                // The client uploads: this end receives and reports back.
                Direction::Upload => received(control, interval, |report| {
                    transfer::receive_tcp(tcp, test.interval, report)
                })?,
                Direction::Download => {
                    transfer::send_tcp(tcp, test.seconds, test.interval, interval)?
                }
            }
        }
        Protocol::Udp => {
            let clients = hellos(udp, token, test.streams)?;
            match test.direction {
                Direction::Upload => received(control, interval, |report| {
                    transfer::receive_udp(
                        &[Arc::clone(udp)],
                        test.streams,
                        test.seconds,
                        test.interval,
                        report,
                    )
                })?,
                Direction::Download => {
                    let sockets = clients
                        .into_iter()
                        .map(|client| (Arc::clone(udp), client))
                        .collect();
                    transfer::send_udp(sockets, test.seconds, test.mbps, test.interval, interval)?
                }
            }
        }
    };
    control.say(wire::DONE)?;
    Ok(total)
}

/// Receives with `receive`, reporting each interval and the total to the client as they come,
/// and each interval to `heard` (this end's own screen).
fn received(
    control: &mut Lines,
    mut heard: impl FnMut(&Report),
    receive: impl FnOnce(&mut dyn FnMut(&Report)) -> Result<Report, String>,
) -> Result<Report, String> {
    // A report the client can't take (it quit) ends nothing here: the transfer finishes and the
    // last say fails instead.
    let total = receive(&mut |report| {
        let _ = control.say(&report.line(false));
        heard(report);
    })?;
    control.say(&total.line(true))?;
    Ok(total)
}

/// Waits for `streams` hello datagrams naming `token`: where the client's UDP streams come
/// from, which is where a download is sent to.
fn hellos(udp: &UdpSocket, token: &str, streams: usize) -> Result<Vec<SocketAddr>, String> {
    let mut clients = Vec::new();
    let mut buffer = vec![0u8; DATAGRAM + 64];
    let deadline = Instant::now() + SETUP;
    udp.set_read_timeout(Some(SETUP))
        .map_err(|e| e.to_string())?;
    while clients.len() < streams {
        if Instant::now() > deadline {
            return Err("the client's UDP streams never arrived".to_string());
        }
        let Ok((got, from)) = udp.recv_from(&mut buffer) else {
            continue;
        };
        if let Some(Datagram::Hello(named)) = Datagram::read(&buffer[..got])
            && named == token
            && !clients.contains(&from)
        {
            clients.push(from);
        }
    }
    Ok(clients)
}

/// A number no two tests share, with no crate: the address of a fresh allocation mixed with the
/// clock. Tokens only tell one test's streams from another's on the same server.
fn rand() -> u128 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let fresh = Box::new(0u8);
    now ^ ((&*fresh as *const u8 as usize as u128) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanspeed::runs::{self, Settings};

    /// A server on a free port, what it says, and the settings to reach it.
    fn serving(protocol: Protocol, directions: Vec<Direction>) -> (Receiver<Event>, Settings) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let port = listener.local_addr().expect("a port").port();
        drop(listener);
        let events = serve(port).expect("a free port");
        let settings = Settings {
            host: "127.0.0.1".to_string(),
            server: SocketAddr::from(([127, 0, 0, 1], port)),
            protocol,
            directions,
            streams: 2,
            seconds: Duration::from_millis(300),
            interval: Duration::from_millis(100),
            mbps: 8.0,
        };
        (events, settings)
    }

    /// What the server has said so far.
    fn said(events: &Receiver<Event>) -> Tests {
        let mut tests = Tests::default();
        while let Ok(event) = events.try_recv() {
            tests.apply(event);
        }
        tests
    }

    #[test]
    fn a_tcp_run_measures_both_ways_through_the_server() {
        let (said_by_server, settings) =
            serving(Protocol::Tcp, vec![Direction::Upload, Direction::Download]);
        let mut events = 0;
        let runs = runs::finished(settings, 1, Duration::ZERO, |event| {
            if let runs::Event::Interval(..) = event {
                events += 1;
            }
        });
        let run = &runs.runs[0];
        assert_eq!(run.failed, None, "{:?}", run.failed);
        assert!(run.upload_mbps().unwrap_or_default() > 1.0);
        assert!(run.download_mbps().unwrap_or_default() > 1.0);
        assert!(events >= 6, "{events} intervals");
        assert!(runs.done);

        // The server saw the same two tests, counted its own intervals, and is free again.
        let tests = said(&said_by_server);
        assert_eq!(tests.tests.len(), 2);
        let (received, sent) = (&tests.tests[0], &tests.tests[1]);
        assert_eq!(received.from, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(received.test.direction, Direction::Upload);
        assert_eq!(received.did(), "received");
        assert_eq!(
            received.total.as_ref().map(|t| t.bytes),
            run.upload.as_ref().map(|t| t.bytes)
        );
        assert_eq!(sent.did(), "sent");
        assert!(
            sent.intervals.len() >= 3,
            "{} intervals sent",
            sent.intervals.len()
        );
        assert!(sent.total.as_ref().is_some_and(|t| t.mbps() > 1.0));
        assert!(!sent.under_way());
        assert_eq!(
            tests.intervals(Direction::Download).len(),
            sent.intervals.len()
        );
    }

    #[test]
    fn a_udp_run_counts_its_datagrams_both_ways() {
        let (_events, settings) =
            serving(Protocol::Udp, vec![Direction::Upload, Direction::Download]);
        let runs = runs::finished(settings, 1, Duration::ZERO, |_| {});
        let run = &runs.runs[0];
        assert_eq!(run.failed, None, "{:?}", run.failed);
        let up = run.upload.as_ref().expect("an upload total");
        let down = run.download.as_ref().expect("a download total");
        assert!(up.datagrams.expect("udp counts").got > 100);
        assert!(down.datagrams.expect("udp counts").got > 100);
        assert_eq!(run.loss(), Some(0.0));
        assert!((6.0..=9.0).contains(&up.mbps()), "{} Mbps up", up.mbps());
    }

    #[test]
    fn nobody_serving_fails_the_run_plainly() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let server = listener.local_addr().expect("a port");
        drop(listener);
        let settings = Settings {
            host: "127.0.0.1".to_string(),
            server,
            protocol: Protocol::Tcp,
            directions: vec![Direction::Upload],
            streams: 1,
            seconds: Duration::from_millis(100),
            interval: Duration::from_millis(100),
            mbps: 0.0,
        };
        let runs = runs::finished(settings, 1, Duration::ZERO, |_| {});
        let why = runs.runs[0].failed.as_deref().unwrap_or_default();
        assert!(why.starts_with("can't reach 127.0.0.1:"), "{why}");
    }
}
