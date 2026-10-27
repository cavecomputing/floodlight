//! Entry point: reads the command line, picks the feature, and opens its screen (or prints its
//! table when asked for plain output, or writes its report).

mod brand;
mod os;
mod ping;
mod report;
mod scan;
mod speedtest;
mod ui;
mod wifi;

use anyhow::bail;
use clap::{ArgGroup, Parser};
use std::path::PathBuf;
use std::time::Duration;

/// Looks at the network around this machine. Pick a feature with its flag.
#[derive(Parser)]
#[command(name = brand::BIN, version, about, arg_required_else_help = true)]
#[command(group(ArgGroup::new("feature").required(true).args(["sweep", "wireless", "speedtest", "ping"])))]
struct Args {
    /// Sweep a subnet: every address that answers, with its MAC, maker and name.
    #[arg(long)]
    sweep: bool,
    /// Survey the WiFi here, scanning until you quit: every network heard, how its signal moves,
    /// how the channels overlap, the quietest one.
    #[arg(long)]
    wireless: bool,
    /// Test the internet connection against Cloudflare: download, upload, latency and jitter.
    #[arg(long)]
    speedtest: bool,
    /// Ping a host over and over: every reply and its round trip, the ones lost, min, average,
    /// max and jitter. A name or an address; the gateway when none is given.
    #[arg(long, value_name = "HOST", num_args = 0..=1)]
    ping: Option<Option<String>>,
    /// The subnet to sweep, like 192.168.1.0/24 (an address alone means its /24).
    /// Defaults to the one this machine is on.
    #[arg(conflicts_with_all = ["wireless", "speedtest", "ping"])]
    subnet: Option<String>,
    /// Print the table and exit instead of opening the screen.
    #[arg(long)]
    plain: bool,
    /// Print only the hosts matching this ip, mac, maker or name fragment, then exit.
    #[arg(long, value_name = "QUERY", conflicts_with_all = ["wireless", "speedtest", "ping"])]
    find: Option<String>,
    /// Write the sweep, survey, speed test or pings as a PDF report to this file, then exit.
    #[arg(long, value_name = "FILE", conflicts_with = "plain")]
    report: Option<PathBuf>,
    /// With --wireless --report: how many seconds to keep scanning before writing, so weak
    /// networks that answer late make the report.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 10,
        requires = "report",
        conflicts_with_all = ["sweep", "speedtest", "ping"]
    )]
    scan_for: u64,
    /// With --speedtest: how many times to run it (1 by default). With --ping: how many pings;
    /// 0, the default, keeps pinging until you quit (10 for --report, which needs an end).
    #[arg(long, value_name = "N", conflicts_with_all = ["sweep", "wireless"])]
    count: Option<u32>,
    /// With --speedtest --count: seconds between runs (5 by default). With --ping: milliseconds
    /// between one reply (or timeout) and the next ping (500 by default).
    #[arg(long, value_name = "N", conflicts_with_all = ["sweep", "wireless"])]
    wait: Option<u64>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = tokio::runtime::Runtime::new()?;
    if args.wireless {
        if let Some(path) = &args.report {
            return report::wireless::write(&runtime, path, args.scan_for);
        }
        if args.plain {
            return ui::plain::wireless(&runtime);
        }
        return ui::wireless::run(&runtime);
    }
    if args.speedtest {
        let count = args.count.unwrap_or(1);
        if count == 0 {
            bail!("--count for --speedtest is how many runs: 1 or more");
        }
        let wait = Duration::from_secs(args.wait.unwrap_or(5));
        if let Some(path) = &args.report {
            return report::speedtest::write(path, count, wait);
        }
        if args.plain {
            return ui::plain::speedtest(count, wait);
        }
        return ui::speedtest::run(count, wait);
    }
    if let Some(host) = &args.ping {
        let target = ping::target::choose(host.as_deref())?;
        let wait = Duration::from_millis(args.wait.unwrap_or(500));
        if let Some(path) = &args.report {
            let count = args.count.unwrap_or(10);
            if count == 0 {
                bail!("a report needs pings that end: give --count 1 or more");
            }
            return report::ping::write(path, &target, count, wait);
        }
        let count = args.count.unwrap_or(0);
        if args.plain {
            return ui::plain::ping(&target, count, wait);
        }
        return ui::ping::run(&target, count, wait);
    }
    let subnet = scan::subnet::choose(args.subnet.as_deref())?;
    if let Some(path) = &args.report {
        return report::sweep::write(&runtime, subnet, args.find.as_deref(), path);
    }
    if args.plain || args.find.is_some() {
        return ui::plain::run(&runtime, subnet, args.find.as_deref());
    }
    ui::app::run(&runtime, subnet)
}
