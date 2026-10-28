//! The serving end: listens on one port, TCP and UDP both, and answers each client's hello with
//! a test. It takes one test at a time (a second client is told `busy`), receives or sends as
//! asked, and prints a line per test. There is no screen: it runs until you quit.

use super::runs::Lines;
use super::transfer;
use super::wire::{self, DATAGRAM, Datagram, Direction, Protocol, Report, Test};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a client has to open its streams after its hello.
const SETUP: Duration = Duration::from_secs(5);

/// Tests waiting for their streams, by token: each hands its TCP streams on through a channel.
type Pending = Arc<Mutex<HashMap<String, Sender<TcpStream>>>>;

/// Listens on `port` and serves tests until the program ends.
pub fn serve(port: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| anyhow::anyhow!("can't listen on tcp port {port}: {e}"))?;
    let udp = Arc::new(
        UdpSocket::bind(("0.0.0.0", port))
            .map_err(|e| anyhow::anyhow!("can't listen on udp port {port}: {e}"))?,
    );
    println!("# listening on port {port} · tcp and udp · ^c quits");
    let pending: Pending = Arc::default();
    let busy = Arc::new(AtomicBool::new(false));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let (pending, busy, udp) = (Arc::clone(&pending), Arc::clone(&busy), Arc::clone(&udp));
        std::thread::spawn(move || answer(stream, &pending, &busy, &udp));
    }
    Ok(())
}

/// One connection: a hello starts a test; a data line joins one.
fn answer(stream: TcpStream, pending: &Pending, busy: &AtomicBool, udp: &Arc<UdpSocket>) {
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
            let _ = streams.send(control.into_stream());
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
    let (tx, rx) = channel();
    pending
        .lock()
        .expect("pending tests")
        .insert(token.clone(), tx);
    let time = chrono::Local::now().format("%H:%M:%S").to_string();
    let outcome = control
        .say(&format!("{} token={token}", wire::OK))
        .and_then(|()| run(&test, &token, &mut control, rx, udp));
    pending.lock().expect("pending tests").remove(&token);
    busy.store(false, Ordering::SeqCst);
    let what = format!(
        "{time} · {} · {} · {} · {} stream{} · {} s",
        from.ip(),
        test.protocol.name(),
        test.direction.name(),
        test.streams,
        if test.streams == 1 { "" } else { "s" },
        test.seconds.as_secs_f32()
    );
    match outcome {
        Ok(words) => println!("{what} · {words}"),
        Err(why) => {
            let _ = control.say(&format!("{} {why}", wire::REFUSED));
            println!("{what} · failed: {why}");
        }
    }
}

/// Serves one test and says how it went: what was received (reported to the client as it went)
/// or what was sent.
fn run(
    test: &Test,
    token: &str,
    control: &mut Lines,
    streams: Receiver<TcpStream>,
    udp: &Arc<UdpSocket>,
) -> Result<String, String> {
    let words = match test.protocol {
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
                Direction::Upload => {
                    let total = received(control, |report| {
                        transfer::receive_tcp(tcp, test.interval, report)
                    })?;
                    format!("received {:.1} Mbps", total.mbps())
                }
                Direction::Download => {
                    let sent = transfer::send_tcp(tcp, test.seconds)?;
                    format!("sent {:.1} Mbps", mbps(sent, test.seconds))
                }
            }
        }
        Protocol::Udp => {
            let clients = hellos(udp, token, test.streams)?;
            match test.direction {
                Direction::Upload => {
                    let total = received(control, |report| {
                        transfer::receive_udp(
                            &[Arc::clone(udp)],
                            test.streams,
                            test.seconds,
                            test.interval,
                            report,
                        )
                    })?;
                    format!(
                        "received {:.1} Mbps · {:.1}% lost",
                        total.mbps(),
                        total.loss().unwrap_or_default()
                    )
                }
                Direction::Download => {
                    let sockets = clients
                        .into_iter()
                        .map(|client| (Arc::clone(udp), client))
                        .collect();
                    let sent = transfer::send_udp(sockets, test.seconds, test.mbps)?;
                    format!("sent {:.1} Mbps", mbps(sent, test.seconds))
                }
            }
        }
    };
    control.say(wire::DONE)?;
    Ok(words)
}

/// Receives with `receive`, reporting each interval and the total to the client as they come.
fn received(
    control: &mut Lines,
    receive: impl FnOnce(&mut dyn FnMut(&Report)) -> Result<Report, String>,
) -> Result<Report, String> {
    // A report the client can't take (it quit) ends nothing here: the transfer finishes and the
    // last say fails instead.
    let total = receive(&mut |report| {
        let _ = control.say(&report.line(false));
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

fn mbps(bytes: u64, took: Duration) -> f32 {
    (bytes as f64 * 8.0 / took.as_secs_f64().max(0.001) / 1e6) as f32
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

impl Lines {
    /// The connection itself, for a data stream that only said which test it joins.
    pub fn into_stream(self) -> TcpStream {
        self.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanspeed::runs::{self, Event, Settings};

    /// A server on a free port, and the settings to reach it.
    fn serving(protocol: Protocol, directions: Vec<Direction>) -> Settings {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let port = listener.local_addr().expect("a port").port();
        drop(listener);
        std::thread::spawn(move || serve(port));
        std::thread::sleep(Duration::from_millis(100));
        Settings {
            host: "127.0.0.1".to_string(),
            server: SocketAddr::from(([127, 0, 0, 1], port)),
            protocol,
            directions,
            streams: 2,
            seconds: Duration::from_millis(300),
            interval: Duration::from_millis(100),
            mbps: 8.0,
        }
    }

    #[test]
    fn a_tcp_run_measures_both_ways_through_the_server() {
        let settings = serving(Protocol::Tcp, vec![Direction::Upload, Direction::Download]);
        let mut events = 0;
        let runs = runs::finished(settings, 1, Duration::ZERO, |event| {
            if let Event::Interval(..) = event {
                events += 1;
            }
        });
        let run = &runs.runs[0];
        assert_eq!(run.failed, None, "{:?}", run.failed);
        assert!(run.upload_mbps().unwrap_or_default() > 1.0);
        assert!(run.download_mbps().unwrap_or_default() > 1.0);
        assert!(events >= 6, "{events} intervals");
        assert!(runs.done);
    }

    #[test]
    fn a_udp_run_counts_its_datagrams_both_ways() {
        let settings = serving(Protocol::Udp, vec![Direction::Upload, Direction::Download]);
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
