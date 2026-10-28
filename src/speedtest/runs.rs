//! The speed test itself: latency first, then downloads and uploads taking turns as they grow,
//! the way Cloudflare's own page measures, run as many times as asked with a wait between. The screen
//! hears about it through `Event`s and keeps the results in `Runs`.

use super::cloudflare::{self, Server};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};
use ureq::Agent;

/// Requests timed for latency and jitter.
const PINGS: usize = 20;
/// Cloudflare's page's own sequence: each size in bytes, how many times, downloads and uploads
/// taking turns as they grow.
const SEQUENCE: [(Phase, usize, usize); 11] = [
    (Phase::Download, 100_000, 9),
    (Phase::Download, 1_000_000, 8),
    (Phase::Upload, 100_000, 8),
    (Phase::Upload, 1_000_000, 6),
    (Phase::Download, 10_000_000, 6),
    (Phase::Upload, 10_000_000, 4),
    (Phase::Download, 25_000_000, 4),
    (Phase::Upload, 25_000_000, 4),
    (Phase::Download, 100_000_000, 3),
    (Phase::Upload, 50_000_000, 3),
    (Phase::Download, 250_000_000, 2),
];
/// Once every request of a size takes longer than this, the link is measured that way; bigger
/// sizes would only take longer.
const LONG_ENOUGH: Duration = Duration::from_secs(1);
/// Transfers at once from this size up. One connection seldom fills a fast line (its TCP window
/// and a single thread's TLS cap it well under a gigabit), so the big sizes go over several,
/// the way fast.com and Ookla measure. The size is split between them, so a run moves what
/// Cloudflare's page moves: four times that set off its limit (429) after a few runs.
const STREAMS_FROM: usize = 10_000_000;
pub const STREAMS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Latency,
    Download,
    Upload,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Latency => "latency",
            Phase::Download => "download",
            Phase::Upload => "upload",
        }
    }
}

pub enum Event {
    /// A run began.
    Started,
    Server(Server),
    Phase(Phase),
    Ping(f32),
    /// One download's or upload's speed, in Mbps.
    Download(f32),
    Upload(f32),
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
    pub server: Option<Server>,
    /// What it is measuring now; None once it is over.
    pub phase: Option<Phase>,
    pub pings: Vec<f32>,
    pub downloads: Vec<f32>,
    pub uploads: Vec<f32>,
    pub failed: Option<String>,
}

impl Run {
    /// The middle round trip, in ms.
    pub fn latency(&self) -> Option<f32> {
        percentile(&self.pings, 0.5)
    }

    /// How much one round trip differs from the next, on average, in ms.
    pub fn jitter(&self) -> Option<f32> {
        jitter(&self.pings)
    }

    /// The 90th percentile of the transfers, as Cloudflare reports it: the speed the link
    /// reaches once a transfer is big enough to fill it, without the odd lucky one.
    pub fn download(&self) -> Option<f32> {
        percentile(&self.downloads, 0.9)
    }

    pub fn upload(&self) -> Option<f32> {
        percentile(&self.uploads, 0.9)
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
            Event::Server(server) => run.server = Some(server),
            Event::Phase(phase) => run.phase = Some(phase),
            Event::Ping(ms) => run.pings.push(ms),
            Event::Download(mbps) => run.downloads.push(mbps),
            Event::Upload(mbps) => run.uploads.push(mbps),
            Event::Finished => run.phase = None,
            Event::Failed(why) => {
                run.phase = None;
                run.failed = Some(why);
            }
            Event::Started | Event::Waiting(_) | Event::Done => {}
        }
    }

    /// The median of one result across the runs that measured it. A failed run's partial
    /// numbers (one cut short by Cloudflare's limit, say) are left out.
    pub fn median(&self, result: fn(&Run) -> Option<f32>) -> Option<f32> {
        let values: Vec<f32> = self
            .runs
            .iter()
            .filter(|run| run.failed.is_none())
            .filter_map(result)
            .collect();
        percentile(&values, 0.5)
    }

    /// The server the runs reached, from the last that said.
    pub fn server(&self) -> Option<&Server> {
        self.runs.iter().rev().find_map(|run| run.server.as_ref())
    }
}

/// How much one round trip differs from the next, on average: the average step between
/// neighbours. `--ping` measures it the same way.
pub fn jitter(round_trips: &[f32]) -> Option<f32> {
    let steps: Vec<f32> = round_trips
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .collect();
    (!steps.is_empty()).then(|| steps.iter().sum::<f32>() / steps.len() as f32)
}

/// The value `share` of the way up the sorted values (0.5 is the median), part way between the
/// two nearest when it falls between them, as Cloudflare's page works it out.
fn percentile(values: &[f32], share: f32) -> Option<f32> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let place = sorted.len().checked_sub(1)? as f32 * share;
    let below = sorted[place.floor() as usize];
    let above = sorted[place.ceil() as usize];
    Some(below + (above - below) * place.fract())
}

/// The top of a chart's scale: 1, 2 or 5 times a power of ten, at or just above `most`.
pub fn scale_top(most: f32) -> f32 {
    if most <= 0.0 {
        return 1.0;
    }
    // Divided below 1 rather than multiplied, so the label reads 0.05 and not 0.049999997.
    let power = most.log10().floor() as i32;
    let times_power = |times: f32| match power {
        0.. => times * 10f32.powi(power),
        _ => times / 10f32.powi(-power),
    };
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(times_power)
        .find(|top| *top >= most)
        .unwrap_or(times_power(10.0))
}

/// Starts `count` runs, `wait` apart, on a thread of their own, and hands back the stream of
/// results. The thread ends with the program, so quitting mid-transfer doesn't wait for it.
pub fn start(count: u32, wait: Duration) -> Receiver<Event> {
    let (tx, rx) = channel();
    std::thread::spawn(move || run_all(count, wait, tx));
    rx
}

/// Runs them all and hands back the results once the last is in, for the outputs that print.
/// `each` hears about every run as it ends.
pub fn finished(count: u32, wait: Duration, mut each: impl FnMut(&Run)) -> Runs {
    let mut runs = Runs::default();
    for event in start(count, wait) {
        let ended = matches!(event, Event::Finished | Event::Failed(_));
        runs.apply(event);
        if ended && let Some(run) = runs.runs.last() {
            each(run);
        }
    }
    runs
}

fn run_all(count: u32, wait: Duration, tx: Sender<Event>) {
    let agent = cloudflare::agent();
    for i in 0..count {
        if i > 0 {
            let _ = tx.send(Event::Waiting(wait));
            std::thread::sleep(wait);
        }
        let _ = tx.send(Event::Started);
        let outcome = run(&agent, &tx);
        // Cloudflare's limit is on this machine for a while, so the runs after would only be
        // refused too.
        let limited = matches!(&outcome, Err(why) if why.starts_with(cloudflare::LIMITED));
        let _ = tx.send(match outcome {
            Ok(()) => Event::Finished,
            Err(why) => Event::Failed(why),
        });
        if limited {
            break;
        }
    }
    let _ = tx.send(Event::Done);
}

fn run(agent: &Agent, tx: &Sender<Event>) -> Result<(), String> {
    let send = |event| {
        let _ = tx.send(event);
    };
    // Where it answered from is nice to know but not needed to measure.
    if let Some(server) = cloudflare::server(agent) {
        send(Event::Server(server));
    }

    send(Event::Phase(Phase::Latency));
    // Nothing to fetch, so each request's time is the round trip. The connection is already
    // open from asking where it answered from; the first one opens it if not, and is left out.
    cloudflare::download(agent, 0)?;
    for _ in 0..PINGS {
        let took = cloudflare::download(agent, 0)?;
        send(Event::Ping(took.as_secs_f32() * 1000.0));
    }

    let mut measured = Vec::new();
    let (mut downloads, mut uploads) = (0, 0);
    let mut connections_open = false;
    for (phase, bytes, times) in SEQUENCE {
        if measured.contains(&phase) {
            continue;
        }
        send(Event::Phase(phase));
        let streams = if bytes >= STREAMS_FROM { STREAMS } else { 1 };
        let body = match phase {
            // The character 0 over and over, the text Cloudflare's page sends.
            Phase::Upload => vec![b'0'; bytes / streams],
            _ => Vec::new(),
        };
        if streams > 1 && !connections_open {
            // Opens the connections the split transfers share, untimed, so no transfer counts
            // their setup.
            transfer(agent, Phase::Download, 0, &[], streams)?;
            connections_open = true;
        }
        let mut quickest = Duration::MAX;
        for _ in 0..times {
            let took = match transfer(agent, phase, bytes / streams, &body, streams) {
                Ok(took) => took,
                // Cloudflare's firewall sometimes refuses a transfer partway through a run,
                // whichever way it goes. What that direction measured so far stands, and the
                // other carries on; only a refusal before anything was measured fails the run.
                // Its limit (429) fails the run: it holds for both directions.
                Err(why) if why.starts_with(cloudflare::REFUSED) => {
                    let some = match phase {
                        Phase::Upload => uploads > 0,
                        _ => downloads > 0,
                    };
                    if !some {
                        return Err(why);
                    }
                    measured.push(phase);
                    break;
                }
                Err(why) => return Err(why),
            };
            match phase {
                Phase::Upload => uploads += 1,
                _ => downloads += 1,
            }
            quickest = quickest.min(took);
            match (phase, cloudflare::mbps(bytes, took)) {
                (Phase::Upload, Some(mbps)) => send(Event::Upload(mbps)),
                (_, Some(mbps)) => send(Event::Download(mbps)),
                (_, None) => {}
            }
        }
        if quickest > LONG_ENOUGH {
            measured.push(phase);
        }
    }
    Ok(())
}

/// One transfer of `bytes` each over `streams` connections at once, and how long the slowest
/// took. A refusal (403) stands, as in Cloudflare's own engine: asking again on fresh
/// connections only added load and lost the cookies Cloudflare had set.
fn transfer(
    agent: &Agent,
    phase: Phase,
    bytes: usize,
    body: &[u8],
    streams: usize,
) -> Result<Duration, String> {
    let one = || match phase {
        Phase::Upload => cloudflare::upload(agent, body),
        _ => cloudflare::download(agent, bytes),
    };
    // Each stream on a thread of its own; the agent they share opens a connection for each.
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..streams).map(|_| scope.spawn(one)).collect();
        let mut slowest = Duration::ZERO;
        for thread in threads {
            let took = thread
                .join()
                .map_err(|_| "a transfer's thread died".to_string())??;
            slowest = slowest.max(took);
        }
        Ok(slowest)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_middle_and_the_ninetieth() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 0.5), Some(2.0));
        let speeds: Vec<f32> = (1..=11).map(|n| n as f32 * 10.0).collect();
        assert_eq!(percentile(&speeds, 0.9), Some(100.0));
        // An even count's middle is halfway between the two middle values.
        assert_eq!(percentile(&[250.0, 200.0], 0.5), Some(225.0));
    }

    #[test]
    fn rounds_the_top_of_a_scale_up() {
        assert_eq!(scale_top(0.0), 1.0);
        assert_eq!(scale_top(241.3), 500.0);
        assert_eq!(scale_top(180.0), 200.0);
        assert_eq!(scale_top(12.5), 20.0);
        assert_eq!(scale_top(100.0), 100.0);
        // Loopback pings: the label has to print as written.
        assert_eq!(scale_top(0.03).to_string(), "0.05");
        assert_eq!(scale_top(0.004).to_string(), "0.005");
    }

    #[test]
    fn jitter_is_the_average_step_between_round_trips() {
        let run = Run {
            pings: vec![10.0, 12.0, 11.0, 15.0],
            ..Run::default()
        };
        assert_eq!(run.latency(), Some(11.5));
        assert_eq!(run.jitter(), Some(7.0 / 3.0));
        assert_eq!(Run::default().jitter(), None);
    }

    #[test]
    fn fills_each_run_from_its_events() {
        let mut runs = Runs::default();
        for event in [
            Event::Started,
            Event::Phase(Phase::Latency),
            Event::Ping(10.0),
            Event::Phase(Phase::Download),
            Event::Download(200.0),
            Event::Finished,
            Event::Waiting(Duration::from_secs(5)),
            Event::Started,
            // Cut short by Cloudflare's limit: what it got this far reads low.
            Event::Download(80.0),
            Event::Failed("cloudflare answered 429".to_string()),
            Event::Done,
        ] {
            runs.apply(event);
        }
        assert_eq!(runs.runs.len(), 2);
        assert_eq!(runs.runs[0].download(), Some(200.0));
        assert_eq!(runs.runs[0].phase, None);
        assert_eq!(runs.runs[1].phase, None);
        assert!(runs.runs[1].failed.is_some());
        assert_eq!(runs.median(Run::latency), Some(10.0));
        assert_eq!(runs.median(Run::download), Some(200.0));
        assert!(runs.done);
    }
}
