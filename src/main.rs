//! Entry point: reads the command line, picks the feature, and opens its screen (or prints its
//! table when asked for plain output, or writes its report).

mod brand;
mod dns;
mod os;
mod ping;
mod report;
mod scan;
mod speedtest;
mod ui;
mod wifi;

use anyhow::bail;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

/// Looks at the network around this machine. Pick a mode; `<mode> --help` shows that mode's own
/// options.
// Each mode is a clap subcommand, so `ping --help` has a page of its own.
#[derive(Parser)]
#[command(name = brand::BIN, version, about, arg_required_else_help = true)]
#[command(
    disable_help_subcommand = true,
    subcommand_help_heading = "Modes",
    subcommand_value_name = "MODE"
)]
#[command(styles = ui::theme::HELP)]
struct Args {
    #[command(subcommand)]
    mode: Mode,
    /// Print the table and exit instead of opening the screen.
    #[arg(long, global = true)]
    plain: bool,
    /// Write the result as a PDF report to this file, then exit.
    #[arg(long, value_name = "FILE", global = true, conflicts_with = "plain")]
    report: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Mode {
    /// Sweep a subnet: every address that answers, with its MAC, maker and name.
    Sweep {
        /// The subnet to sweep, like 192.168.1.0/24 (an address alone means its /24).
        /// Defaults to the one this machine is on.
        subnet: Option<String>,
        /// Print only the hosts matching this ip, mac, maker or name fragment, then exit.
        #[arg(long, value_name = "QUERY")]
        find: Option<String>,
    },
    /// Survey the WiFi here: every network heard, its signal and how the channels overlap.
    Wireless {
        /// With --report: how many seconds to keep scanning before writing, so weak networks
        /// that answer late make the report.
        #[arg(long, value_name = "SECONDS", default_value_t = 10)]
        scan_for: u64,
    },
    /// Test the internet connection against Cloudflare: download, upload, latency and jitter.
    Speedtest {
        /// How many times to run it.
        #[arg(long, value_name = "N", default_value_t = 1)]
        count: u32,
        /// Seconds between runs.
        #[arg(long, value_name = "SECONDS", default_value_t = 5)]
        wait: u64,
    },
    /// Ping up to five hosts over and over: every round trip, the ones lost, min, average, max
    /// and jitter.
    Ping {
        /// Up to five names or IPv4 addresses, pinged side by side. The gateway when none is given.
        #[arg(value_name = "HOST", num_args = 0..=5)]
        hosts: Vec<String>,
        /// How many pings. 0, the default, keeps pinging until you quit (10 for --report, which
        /// needs an end).
        #[arg(long, value_name = "N")]
        count: Option<u32>,
        /// Milliseconds between one reply (or a timeout) and the next ping.
        #[arg(long, value_name = "MS", default_value_t = 500)]
        wait: u64,
    },
    /// Look a name up in DNS: its records, their TTLs, how long the answer took, and whether it
    /// came from the domain's own name server. An address is looked up for its name.
    Dns {
        /// The name to look up, like example.com, or an address for its name (a reverse lookup).
        name: String,
        /// The record types to ask for, like a, aaaa, mx, txt, ns, cname, soa, srv, caa or any;
        /// several with commas. A by default, or PTR for an address.
        #[arg(long = "type", value_name = "TYPE", value_delimiter = ',')]
        types: Vec<String>,
        /// The name servers to ask, by address (a port after a colon) or name; several with
        /// commas, each asked in turn. The one this machine uses by default.
        #[arg(long, value_name = "SERVER", value_delimiter = ',')]
        server: Vec<String>,
        /// Ask the domain's own name server (found through the usual one) for an authoritative
        /// answer, past any cache.
        #[arg(long)]
        authoritative: bool,
        /// Ask over TCP instead of UDP.
        #[arg(long)]
        tcp: bool,
        /// Also ask whois who registered the domain, or who holds the address.
        #[arg(long)]
        whois: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = tokio::runtime::Runtime::new()?;
    match args.mode {
        Mode::Sweep { subnet, find } => {
            let subnet = scan::subnet::choose(subnet.as_deref())?;
            if let Some(path) = &args.report {
                return report::sweep::write(&runtime, subnet, find.as_deref(), path);
            }
            if args.plain || find.is_some() {
                return ui::plain::run(&runtime, subnet, find.as_deref());
            }
            ui::app::run(&runtime, subnet)
        }
        Mode::Wireless { scan_for } => {
            if let Some(path) = &args.report {
                return report::wireless::write(&runtime, path, scan_for);
            }
            if args.plain {
                return ui::plain::wireless(&runtime);
            }
            ui::wireless::run(&runtime)
        }
        Mode::Speedtest { count, wait } => {
            if count == 0 {
                bail!("--count for speedtest is how many runs: 1 or more");
            }
            let wait = Duration::from_secs(wait);
            if let Some(path) = &args.report {
                return report::speedtest::write(path, count, wait);
            }
            if args.plain {
                return ui::plain::speedtest(count, wait);
            }
            ui::speedtest::run(count, wait)
        }
        Mode::Ping { hosts, count, wait } => {
            let targets = match hosts.as_slice() {
                [] => vec![ping::target::choose(None)?],
                hosts => hosts
                    .iter()
                    .map(|host| ping::target::choose(Some(host)))
                    .collect::<anyhow::Result<_>>()?,
            };
            let wait = Duration::from_millis(wait);
            if let Some(path) = &args.report {
                let count = count.unwrap_or(10);
                if count == 0 {
                    bail!("a report needs pings that end: give --count 1 or more");
                }
                return report::ping::write(path, &targets, count, wait);
            }
            let count = count.unwrap_or(0);
            if args.plain {
                return ui::plain::ping(&targets, count, wait);
            }
            ui::ping::run(&targets, count, wait)
        }
        Mode::Dns {
            name,
            types,
            server,
            authoritative,
            tcp,
            whois,
        } => {
            let lookups = dns::lookup::lookups(&name, &types, &server, authoritative, tcp)?;
            let whois = whois.then(|| dns::whois::whois(&name));
            if let Some(path) = &args.report {
                return report::dns::write(path, &lookups, whois.as_ref());
            }
            ui::dns::print(&lookups, whois.as_ref(), args.plain);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_line_is_consistent() {
        Args::command().debug_assert();
    }

    #[test]
    fn modes_are_typed_as_flags() {
        let args = Args::try_parse_from([
            brand::BIN,
            "--plain",
            "ping",
            "1.1.1.1",
            "192.168.1.1",
            "--wait",
            "250",
        ])
        .expect("a ping with its options");
        assert!(args.plain);
        assert!(matches!(
            args.mode,
            Mode::Ping {
                ref hosts,
                count: None,
                wait: 250
            } if hosts.len() == 2
        ));
        let six = [
            "ping", "1.1.1.1", "1.1.1.2", "1.1.1.3", "1.1.1.4", "1.1.1.5", "1.1.1.6",
        ];
        assert!(Args::try_parse_from([brand::BIN].into_iter().chain(six)).is_err());
        assert!(Args::try_parse_from([brand::BIN, "wireless", "--count", "3"]).is_err());
        assert!(
            Args::try_parse_from([brand::BIN, "sweep", "--report", "a.pdf", "--plain"]).is_err()
        );
        let dns = [
            "dns",
            "example.com",
            "--type",
            "a,mx",
            "--server",
            "1.1.1.1",
            "--whois",
        ];
        let args = Args::try_parse_from([brand::BIN].into_iter().chain(dns)).expect("a lookup");
        assert!(matches!(
            args.mode,
            Mode::Dns { ref types, whois: true, .. } if types == &["a", "mx"]
        ));
    }
}
