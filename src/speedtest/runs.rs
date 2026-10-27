//! The speed test itself: latency first, then download and upload in growing transfers, the way
//! Cloudflare's own page measures, run as many times as asked with a wait between. The screen
//! hears about it through `Event`s and keeps the results in `Runs`.

use super::cloudflare::{self, Server, Transfer};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Requests timed for latency and jitter.
const PINGS: usize = 20;
/// Download sizes in bytes, each fetched a few times, smallest first.
const DOWNLOADS: [(u64, usize); 5] = [
    (100_000, 4),
    (1_000_000, 4),
    (10_000_000, 3),
    (25_000_000, 2),
    (100_000_000, 2),
];
const UPLOADS: [(usize, usize); 4] = [
    (100_000, 4),
    (1_000_000, 4),
    (10_000_000, 3),
    (25_000_000, 2),
];
/// Once a size takes this long the link is measured; bigger ones would only take longer.
const LONG_ENOUGH: Duration = Duration::from_secs(1);

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
        let steps: Vec<f32> = self.pings.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        (!steps.is_empty()).then(|| steps.iter().sum::<f32>() / steps.len() as f32)
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

    /// The median of one result across the runs that measured it.
    pub fn median(&self, result: fn(&Run) -> Option<f32>) -> Option<f32> {
        let values: Vec<f32> = self.runs.iter().filter_map(result).collect();
        percentile(&values, 0.5)
    }

    /// The server the runs reached, from the last that said.
    pub fn server(&self) -> Option<&Server> {
        self.runs.iter().rev().find_map(|run| run.server.as_ref())
    }
}

/// The value `share` of the way up the sorted values (0.5 is the median).
fn percentile(values: &[f32], share: f32) -> Option<f32> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let last = sorted.len().checked_sub(1)?;
    Some(sorted[(last as f32 * share).round() as usize])
}

/// Starts `count` runs on the runtime, `wait` apart, and hands back the stream of results.
pub fn start(runtime: &Runtime, count: u32, wait: Duration) -> UnboundedReceiver<Event> {
    let (tx, rx) = unbounded_channel();
    runtime.spawn(run_all(count, wait, tx));
    rx
}

/// Runs them all and hands back the results once the last is in, for the outputs that print.
/// `each` hears about every run as it ends.
pub fn finished(runtime: &Runtime, count: u32, wait: Duration, mut each: impl FnMut(&Run)) -> Runs {
    let mut events = start(runtime, count, wait);
    let mut runs = Runs::default();
    runtime.block_on(async {
        while let Some(event) = events.recv().await {
            let ended = matches!(event, Event::Finished | Event::Failed(_));
            let done = matches!(event, Event::Done);
            runs.apply(event);
            if ended && let Some(run) = runs.runs.last() {
                each(run);
            }
            if done {
                break;
            }
        }
    });
    runs
}

async fn run_all(count: u32, wait: Duration, tx: UnboundedSender<Event>) {
    for i in 0..count {
        if i > 0 {
            let _ = tx.send(Event::Waiting(wait));
            tokio::time::sleep(wait).await;
        }
        let _ = tx.send(Event::Started);
        let _ = tx.send(match run(&tx).await {
            Ok(()) => Event::Finished,
            Err(why) => Event::Failed(why),
        });
    }
    let _ = tx.send(Event::Done);
}

async fn run(tx: &UnboundedSender<Event>) -> Result<(), String> {
    let send = |event| {
        let _ = tx.send(event);
    };
    send(Event::Server(cloudflare::server().await?));

    send(Event::Phase(Phase::Latency));
    // Nothing to fetch, so each request's time to the first byte is the round trip.
    cloudflare::download(0, PINGS, |t| send(Event::Ping(t.latency_ms()))).await?;

    send(Event::Phase(Phase::Download));
    for (bytes, times) in DOWNLOADS {
        let mut long_enough = false;
        cloudflare::download(bytes, times, |t: Transfer| {
            long_enough |= t.took() >= LONG_ENOUGH;
            if let Some(mbps) = t.mbps(t.downloaded) {
                send(Event::Download(mbps));
            }
        })
        .await?;
        if long_enough {
            break;
        }
    }

    send(Event::Phase(Phase::Upload));
    for (bytes, times) in UPLOADS {
        let mut long_enough = false;
        cloudflare::upload(bytes, times, |t: Transfer| {
            long_enough |= t.took() >= LONG_ENOUGH;
            if let Some(mbps) = t.mbps(t.uploaded) {
                send(Event::Upload(mbps));
            }
        })
        .await?;
        if long_enough {
            break;
        }
    }
    Ok(())
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
    }

    #[test]
    fn jitter_is_the_average_step_between_round_trips() {
        let run = Run {
            pings: vec![10.0, 12.0, 11.0, 15.0],
            ..Run::default()
        };
        assert_eq!(run.latency(), Some(12.0));
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
        assert!(runs.done);
    }
}
