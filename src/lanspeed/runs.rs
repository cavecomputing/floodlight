//! The client's side: a run is a transfer each way asked for (upload, then download), run as
//! many times as asked with a wait between. The screen hears about it through `Event`s and
//! keeps the results in `Runs`, the way the speed test's screen does.

use super::transfer;
use super::wire::{self, Datagram, Direction, Protocol, Report, Test};
use crate::speedtest::runs::percentile;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// How long the server has to answer a hello, and each stream to be let in.
const CONNECT: Duration = Duration::from_secs(5);

/// Everything a run needs: where the server is and what to ask it for.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The host as typed, for the headline.
    pub host: String,
    pub server: SocketAddr,
    pub protocol: Protocol,
    /// The directions in turn: both, or the one asked for.
    pub directions: Vec<Direction>,
    pub streams: usize,
    pub seconds: Duration,
    pub interval: Duration,
    /// UDP's sending rate in Mbps (0: as fast as it can).
    pub mbps: f32,
}

impl Settings {
    fn test(&self, direction: Direction) -> Test {
        Test {
            protocol: self.protocol,
            direction,
            streams: self.streams,
            seconds: self.seconds,
            interval: self.interval,
            mbps: self.mbps,
        }
    }
}

pub enum Event {
    /// A run began.
    Started,
    /// A transfer began, one way.
    Phase(Direction),
    /// What arrived over one interval.
    Interval(Direction, Report),
    /// What arrived over a whole transfer.
    Total(Direction, Report),
    /// The run is over: measured, or it failed.
    Finished,
    Failed(String),
    /// The run before is over; the next starts after this long.
    Waiting(Duration),
    /// Every run is over.
    Done,
}

/// One run's results, filled in as they come.
#[derive(Default)]
pub struct Run {
    /// When it started, like `14:05:09`.
    pub time: String,
    /// The transfer under way; None once the run is over.
    pub phase: Option<Direction>,
    pub intervals: Vec<(Direction, Report)>,
    pub upload: Option<Report>,
    pub download: Option<Report>,
    pub failed: Option<String>,
}

impl Run {
    pub fn total(&self, direction: Direction) -> Option<&Report> {
        match direction {
            Direction::Upload => self.upload.as_ref(),
            Direction::Download => self.download.as_ref(),
        }
    }

    /// The whole transfer's speed one way, in Mbps.
    pub fn mbps(&self, direction: Direction) -> Option<f32> {
        self.total(direction).map(Report::mbps)
    }

    pub fn upload_mbps(&self) -> Option<f32> {
        self.mbps(Direction::Upload)
    }

    pub fn download_mbps(&self) -> Option<f32> {
        self.mbps(Direction::Download)
    }

    /// UDP: the share of datagrams lost, both ways together, as a percentage.
    pub fn loss(&self) -> Option<f32> {
        let counts: Vec<wire::Datagrams> = [self.upload.as_ref(), self.download.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|report| report.datagrams)
            .collect();
        let got: u64 = counts.iter().map(|d| d.got).sum();
        let lost: u64 = counts.iter().map(|d| d.lost).sum();
        (!counts.is_empty()).then(|| match got + lost {
            0 => 0.0,
            sent => lost as f32 * 100.0 / sent as f32,
        })
    }

    /// UDP: the jitter, averaged over the directions measured, in ms.
    pub fn jitter(&self) -> Option<f32> {
        let jitters: Vec<f32> = [self.upload.as_ref(), self.download.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|report| report.datagrams.map(|d| d.jitter_ms))
            .collect();
        (!jitters.is_empty()).then(|| jitters.iter().sum::<f32>() / jitters.len() as f32)
    }
}

/// Every run so far.
#[derive(Default)]
pub struct Runs {
    pub runs: Vec<Run>,
    /// When the next run starts, while waiting between runs.
    pub next: Option<Instant>,
    pub done: bool,
}

impl Runs {
    pub fn apply(&mut self, event: Event) {
        if let Event::Started = event {
            self.next = None;
            self.runs.push(Run {
                time: chrono::Local::now().format("%H:%M:%S").to_string(),
                ..Run::default()
            });
            return;
        }
        if let Event::Waiting(wait) = event {
            self.next = Some(Instant::now() + wait);
            return;
        }
        if let Event::Done = event {
            self.done = true;
            return;
        }
        let Some(run) = self.runs.last_mut() else {
            return;
        };
        match event {
            Event::Phase(direction) => run.phase = Some(direction),
            Event::Interval(direction, report) => run.intervals.push((direction, report)),
            Event::Total(Direction::Upload, report) => run.upload = Some(report),
            Event::Total(Direction::Download, report) => run.download = Some(report),
            Event::Finished => run.phase = None,
            Event::Failed(why) => {
                run.phase = None;
                run.failed = Some(why);
            }
            Event::Started | Event::Waiting(_) | Event::Done => {}
        }
    }

    /// The median of one result across the runs that measured it. A failed run's partial
    /// numbers are left out.
    pub fn median(&self, result: fn(&Run) -> Option<f32>) -> Option<f32> {
        let values: Vec<f32> = self
            .runs
            .iter()
            .filter(|run| run.failed.is_none())
            .filter_map(result)
            .collect();
        percentile(&values, 0.5)
    }

    /// Every interval measured so far one way, across the runs, for the graphs.
    pub fn intervals(&self, direction: Direction) -> Vec<Option<f32>> {
        self.runs
            .iter()
            .flat_map(|run| run.intervals.iter())
            .filter(|(way, _)| *way == direction)
            .map(|(_, report)| Some(report.mbps()))
            .collect()
    }
}

/// Starts `count` runs, `wait` apart, on a thread of their own, and hands back the stream of
/// results. The thread ends with the program, so quitting mid-transfer doesn't wait for it.
pub fn start(settings: Settings, count: u32, wait: Duration) -> Receiver<Event> {
    let (tx, rx) = channel();
    std::thread::spawn(move || run_all(&settings, count, wait, tx));
    rx
}

/// Runs them all and hands back the results once the last is in, for the outputs that print.
/// `each` hears about every event as it comes.
pub fn finished(
    settings: Settings,
    count: u32,
    wait: Duration,
    mut each: impl FnMut(&Event),
) -> Runs {
    let mut runs = Runs::default();
    for event in start(settings, count, wait) {
        each(&event);
        runs.apply(event);
    }
    runs
}

fn run_all(settings: &Settings, count: u32, wait: Duration, tx: Sender<Event>) {
    for i in 0..count {
        if i > 0 {
            let _ = tx.send(Event::Waiting(wait));
            std::thread::sleep(wait);
        }
        let _ = tx.send(Event::Started);
        let mut outcome = Ok(());
        for direction in &settings.directions {
            let _ = tx.send(Event::Phase(*direction));
            outcome = transfer(settings, *direction, &tx);
            if outcome.is_err() {
                break;
            }
        }
        let _ = tx.send(match outcome {
            Ok(()) => Event::Finished,
            Err(why) => Event::Failed(why),
        });
    }
    let _ = tx.send(Event::Done);
}

/// One transfer, one way: asks the server, opens the streams, then sends or receives.
fn transfer(settings: &Settings, direction: Direction, tx: &Sender<Event>) -> Result<(), String> {
    let test = settings.test(direction);
    let control = TcpStream::connect_timeout(&settings.server, CONNECT)
        .map_err(|e| format!("can't reach {}: {e}", settings.server))?;
    control
        .set_read_timeout(Some(CONNECT))
        .map_err(|e| e.to_string())?;
    let mut control = Lines::new(control);
    control.say(&test.hello())?;
    let answer = control.hear()?;
    let (word, fields) = wire::fields(&answer);
    let token = match word {
        wire::OK => fields
            .iter()
            .find(|(key, _)| *key == "token")
            .map(|(_, token)| token.to_string())
            .ok_or("the server's ok named no token")?,
        wire::BUSY => return Err("the server is busy with another test".to_string()),
        _ => return Err(format!("the server refused: {answer}")),
    };
    let data = format!("{} token={token}", wire::DATA);
    let report = |report: &Report| {
        let _ = tx.send(Event::Interval(direction, report.clone()));
    };
    let total = match (settings.protocol, direction) {
        (Protocol::Tcp, _) => {
            let mut streams = Vec::new();
            for _ in 0..settings.streams {
                let mut stream = TcpStream::connect_timeout(&settings.server, CONNECT)
                    .map_err(|e| format!("can't open a stream to {}: {e}", settings.server))?;
                stream
                    .write_all(format!("{data}\n").as_bytes())
                    .map_err(|e| e.to_string())?;
                streams.push(stream);
            }
            match direction {
                Direction::Upload => {
                    // The server counts what arrives and reports it back on the control
                    // connection while this end sends.
                    let reports = std::thread::scope(|scope| {
                        let reports = scope.spawn(|| control.reports(report));
                        transfer::send_tcp(streams, settings.seconds)?;
                        reports.join().map_err(|_| "the reports' thread died")?
                    })?;
                    reports.ok_or("the server never said what arrived")?
                }
                Direction::Download => {
                    let total = transfer::receive_tcp(streams, settings.interval, report)?;
                    control.reports(|_| {})?;
                    total
                }
            }
        }
        (Protocol::Udp, _) => {
            let mut sockets = Vec::new();
            for _ in 0..settings.streams {
                let socket = UdpSocket::bind(unspecified(settings.server))
                    .map_err(|e| format!("can't open a UDP socket: {e}"))?;
                socket
                    .send_to(&Datagram::Hello(&token).bytes(), settings.server)
                    .map_err(|e| format!("can't send to {}: {e}", settings.server))?;
                sockets.push(Arc::new(socket));
            }
            match direction {
                Direction::Upload => {
                    let sockets = sockets
                        .into_iter()
                        .map(|socket| (socket, settings.server))
                        .collect();
                    let reports = std::thread::scope(|scope| {
                        let reports = scope.spawn(|| control.reports(report));
                        transfer::send_udp(sockets, settings.seconds, settings.mbps)?;
                        reports.join().map_err(|_| "the reports' thread died")?
                    })?;
                    reports.ok_or("the server never said what arrived")?
                }
                Direction::Download => {
                    let total = transfer::receive_udp(
                        &sockets,
                        settings.streams,
                        settings.seconds,
                        settings.interval,
                        report,
                    )?;
                    control.reports(|_| {})?;
                    total
                }
            }
        }
    };
    let _ = tx.send(Event::Total(direction, total));
    Ok(())
}

/// The server's address from the host typed (a name or an address) and the port.
pub fn resolve(host: &str, port: u16) -> anyhow::Result<SocketAddr> {
    use std::net::ToSocketAddrs;
    if host.is_empty() {
        anyhow::bail!("--connect needs the host serving, by name or address");
    }
    let mut addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("can't find {host}: {e}"))?;
    // IPv4 first, as the rest of the tool speaks it.
    let addrs: Vec<SocketAddr> = addrs.by_ref().collect();
    addrs
        .iter()
        .find(|addr| addr.is_ipv4())
        .or(addrs.first())
        .copied()
        .ok_or_else(|| anyhow::anyhow!("{host} has no address"))
}

/// The any-address of the server's family, to bind a socket that can reach it.
fn unspecified(server: SocketAddr) -> SocketAddr {
    match server {
        SocketAddr::V4(_) => SocketAddr::from(([0, 0, 0, 0], 0)),
        SocketAddr::V6(_) => SocketAddr::from(([0u16; 8], 0)),
    }
}

/// A control connection, read a line at a time. Both ends use it.
pub struct Lines {
    reader: BufReader<TcpStream>,
}

impl Lines {
    pub fn new(stream: TcpStream) -> Lines {
        Lines {
            reader: BufReader::new(stream),
        }
    }

    /// The connection itself, for a data stream that only said which test it joins.
    pub fn into_inner(self) -> TcpStream {
        self.reader.into_inner()
    }

    pub fn say(&mut self, line: &str) -> Result<(), String> {
        self.reader
            .get_mut()
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| format!("the other end went away: {e}"))
    }

    /// The next line, without its newline.
    pub fn hear(&mut self) -> Result<String, String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err("the other end hung up".to_string()),
            Ok(_) => Ok(line.trim_end().to_string()),
            Err(e) => Err(format!("the other end went quiet: {e}")),
        }
    }

    /// Hears the receiver's reports until `done`, handing each interval on and keeping the
    /// total. A transfer can run for minutes, so this waits as long as it takes; the other end
    /// hanging up ends it.
    fn reports(&mut self, mut each: impl FnMut(&Report)) -> Result<Option<Report>, String> {
        self.reader
            .get_mut()
            .set_read_timeout(None)
            .map_err(|e| e.to_string())?;
        let mut total = None;
        loop {
            let line = self.hear()?;
            if line == wire::DONE {
                return Ok(total);
            }
            if line.starts_with(wire::REFUSED) {
                return Err(format!("the server gave up: {line}"));
            }
            let (report, is_total) = Report::from_line(&line)?;
            match is_total {
                true => total = Some(report),
                false => each(&report),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanspeed::wire::Datagrams;

    fn report(bytes: u64, datagrams: Option<Datagrams>) -> Report {
        Report {
            from: 0.0,
            to: 1.0,
            bytes,
            datagrams,
        }
    }

    #[test]
    fn fills_each_run_from_its_events() {
        let mut runs = Runs::default();
        for event in [
            Event::Started,
            Event::Phase(Direction::Upload),
            Event::Interval(Direction::Upload, report(100_000_000, None)),
            Event::Total(Direction::Upload, report(100_000_000, None)),
            Event::Phase(Direction::Download),
            Event::Interval(Direction::Download, report(50_000_000, None)),
            Event::Total(Direction::Download, report(50_000_000, None)),
            Event::Finished,
            Event::Waiting(Duration::from_secs(5)),
            Event::Started,
            Event::Interval(Direction::Upload, report(10_000_000, None)),
            Event::Failed("the server hung up".to_string()),
            Event::Done,
        ] {
            runs.apply(event);
        }
        assert_eq!(runs.runs.len(), 2);
        assert_eq!(runs.runs[0].upload_mbps(), Some(800.0));
        assert_eq!(runs.runs[0].download_mbps(), Some(400.0));
        assert_eq!(runs.runs[0].phase, None);
        assert!(runs.runs[1].failed.is_some());
        assert_eq!(runs.median(Run::upload_mbps), Some(800.0));
        assert_eq!(runs.median(Run::jitter), None);
        assert_eq!(
            runs.intervals(Direction::Upload),
            vec![Some(800.0), Some(80.0)]
        );
        assert!(runs.done);
    }

    #[test]
    fn udp_runs_add_up_their_loss_and_jitter() {
        let mut run = Run::default();
        let counts = |got, lost, jitter_ms| {
            Some(Datagrams {
                got,
                lost,
                jitter_ms,
            })
        };
        run.upload = Some(report(1_400_000, counts(990, 10, 0.5)));
        run.download = Some(report(1_400_000, counts(1000, 0, 0.3)));
        assert_eq!(run.loss(), Some(0.5));
        assert_eq!(run.jitter(), Some(0.4));
        assert_eq!(Run::default().loss(), None);
    }

    #[test]
    fn the_median_is_the_middle_run() {
        let mut runs = Runs::default();
        for mbps in [900.0, 100.0, 500.0, 700.0] {
            runs.apply(Event::Started);
            runs.apply(Event::Total(
                Direction::Upload,
                report((mbps * 1e6 / 8.0) as u64, None),
            ));
            runs.apply(Event::Finished);
        }
        assert_eq!(runs.median(Run::upload_mbps), Some(600.0));
        assert_eq!(Runs::default().median(Run::upload_mbps), None);
    }
}
