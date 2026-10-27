//! Entry point: reads the command line, picks the feature, and opens its screen (or prints its
//! table when asked for plain output, or writes its report).

mod brand;
mod os;
mod report;
mod scan;
mod speedtest;
mod ui;
mod wifi;

use clap::{ArgGroup, Parser};
use std::path::PathBuf;
use std::time::Duration;

/// Looks at the network around this machine. Pick a feature with its flag.
#[derive(Parser)]
#[command(name = brand::BIN, version, about, arg_required_else_help = true)]
#[command(group(ArgGroup::new("feature").required(true).args(["sweep", "wireless", "speedtest"])))]
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
    /// The subnet to sweep, like 192.168.1.0/24 (an address alone means its /24).
    /// Defaults to the one this machine is on.
    #[arg(conflicts_with_all = ["wireless", "speedtest"])]
    subnet: Option<String>,
    /// Print the table and exit instead of opening the screen.
    #[arg(long)]
    plain: bool,
    /// Print only the hosts matching this ip, mac, maker or name fragment, then exit.
    #[arg(long, value_name = "QUERY", conflicts_with_all = ["wireless", "speedtest"])]
    find: Option<String>,
    /// Write the sweep, survey or speed test as a PDF report to this file, then exit.
    #[arg(long, value_name = "FILE", conflicts_with = "plain")]
    report: Option<PathBuf>,
    /// With --wireless --report: how many seconds to keep scanning before writing, so weak
    /// networks that answer late make the report.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 10,
        requires = "report",
        conflicts_with_all = ["sweep", "speedtest"]
    )]
    scan_for: u64,
    /// With --speedtest: how many times to run it.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..),
        conflicts_with_all = ["sweep", "wireless"]
    )]
    count: u32,
    /// With --speedtest --count: how many seconds to wait between runs.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 5,
        conflicts_with_all = ["sweep", "wireless"]
    )]
    wait: u64,
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
        let wait = Duration::from_secs(args.wait);
        if let Some(path) = &args.report {
            return report::speedtest::write(path, args.count, wait);
        }
        if args.plain {
            return ui::plain::speedtest(args.count, wait);
        }
        return ui::speedtest::run(args.count, wait);
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
