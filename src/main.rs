//! Entry point: reads the command line, picks the feature, and opens its screen (or prints its
//! table when asked for plain output, or writes its report).

mod brand;
mod dns;
mod lanspeed;
mod os;
mod ping;
mod report;
mod scan;
mod speedtest;
mod ui;
mod wake;
mod wifi;

use anyhow::bail;
use clap::{Parser, Subcommand};
use std::net::Ipv4Addr;
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
    /// Test the throughput between two machines on the LAN, iperf-style: this tool serving on
    /// one, connecting from the other, over TCP or UDP.
    Lanspeed {
        /// Wait for a client and serve its tests, until you quit.
        #[arg(long, conflicts_with = "connect", required_unless_present = "connect")]
        serve: bool,
        /// Run the test against the machine serving at this name or address.
        #[arg(long, value_name = "HOST")]
        connect: Option<String>,
        /// The port the server listens on (TCP and UDP both) and the client connects to.
        #[arg(long, value_name = "PORT", default_value_t = lanspeed::PORT)]
        port: u16,
        /// Test over TCP: the throughput the connection reaches. The default.
        #[arg(long, conflicts_with = "udp")]
        tcp: bool,
        /// Test over UDP at --bitrate: the datagrams lost and the jitter besides the throughput.
        #[arg(long)]
        udp: bool,
        /// Only send to the server (both ways in turn by default).
        #[arg(long, conflicts_with = "download")]
        upload: bool,
        /// Only receive from the server.
        #[arg(long)]
        download: bool,
        /// Seconds each transfer runs.
        #[arg(long, value_name = "SECONDS", default_value_t = 10.0)]
        time: f32,
        /// Connections sending at once, 1 to 64, when one won't fill the link.
        #[arg(long, value_name = "N", default_value_t = 1)]
        streams: usize,
        /// Seconds between the rows reporting what arrived.
        #[arg(long, value_name = "SECONDS", default_value_t = 1.0)]
        interval: f32,
        /// UDP: how fast to send, in Mbps, all streams together. 0 sends as fast as it can.
        #[arg(long, value_name = "MBPS", default_value_t = 10.0)]
        bitrate: f32,
        /// How many times to run it.
        #[arg(long, value_name = "N", default_value_t = 1)]
        count: u32,
        /// Seconds between runs.
        #[arg(long, value_name = "SECONDS", default_value_t = 5)]
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
    /// Wake a sleeping machine with a Wake-on-LAN magic packet, broadcast to its MAC address.
    Wake {
        /// The MAC addresses to wake, like aa:bb:cc:dd:ee:ff, AA-BB-CC-DD-EE-FF, aabb.ccdd.eeff
        /// or aabbccddeeff.
        #[arg(value_name = "MAC", required = true)]
        macs: Vec<String>,
        /// Where to send it: the broadcast address of the machine's subnet, like 192.168.1.255,
        /// when this machine has several networks, or an address a router forwards from.
        #[arg(long, value_name = "ADDRESS", default_value_t = Ipv4Addr::BROADCAST)]
        broadcast: Ipv4Addr,
        /// The UDP port to send it to (9, the usual one, or 7 for some cards).
        #[arg(long, value_name = "PORT", default_value_t = wake::PORT)]
        port: u16,
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
        Mode::Lanspeed {
            serve,
            connect,
            port,
            tcp: _,
            udp,
            upload,
            download,
            time,
            streams,
            interval,
            bitrate,
            count,
            wait,
        } => {
            if serve {
                if args.report.is_some() {
                    bail!("--serve has nothing to report; --report goes with --connect");
                }
                return lanspeed::server::serve(port);
            }
            if count == 0 {
                bail!("--count for lanspeed is how many runs: 1 or more");
            }
            if !(1..=64).contains(&streams) {
                bail!("--streams is 1 to 64");
            }
            if !(0.1..=3600.0).contains(&time) || !(0.1..=3600.0).contains(&interval) {
                bail!("--time and --interval are 0.1 to 3600 seconds");
            }
            let host = connect.unwrap_or_default();
            let settings = lanspeed::runs::Settings {
                server: lanspeed::runs::resolve(&host, port)?,
                host,
                protocol: match udp {
                    true => lanspeed::wire::Protocol::Udp,
                    false => lanspeed::wire::Protocol::Tcp,
                },
                directions: match (upload, download) {
                    (true, _) => vec![lanspeed::wire::Direction::Upload],
                    (_, true) => vec![lanspeed::wire::Direction::Download],
                    _ => vec![
                        lanspeed::wire::Direction::Upload,
                        lanspeed::wire::Direction::Download,
                    ],
                },
                streams,
                seconds: Duration::from_secs_f32(time),
                interval: Duration::from_secs_f32(interval),
                mbps: bitrate,
            };
            let wait = Duration::from_secs(wait);
            if let Some(path) = &args.report {
                return report::lanspeed::write(path, settings, count, wait);
            }
            if args.plain {
                return ui::plain::lanspeed(settings, count, wait);
            }
            ui::lanspeed::run(settings, count, wait)
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
        Mode::Wake {
            macs,
            broadcast,
            port,
        } => {
            if args.report.is_some() {
                bail!("wake has nothing to report: it sends a packet and nothing answers");
            }
            // Every MAC is read before any is sent, so a typo doesn't leave half of them sent.
            let macs = macs
                .iter()
                .map(|typed| match scan::hosts::normalize_mac(typed) {
                    Some(mac) => Ok(mac),
                    None => bail!("{typed} isn't a MAC address, like aa:bb:cc:dd:ee:ff"),
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            for mac in &macs {
                wake::send(mac, broadcast, port)?;
                println!("sent a wake packet to {mac} · {broadcast}:{port}");
            }
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
        let lanspeed = [
            "lanspeed",
            "--connect",
            "192.168.1.20",
            "--udp",
            "--streams",
            "4",
        ];
        let args = Args::try_parse_from([brand::BIN].into_iter().chain(lanspeed)).expect("a test");
        assert!(matches!(
            args.mode,
            Mode::Lanspeed {
                serve: false,
                udp: true,
                streams: 4,
                port: 5201,
                ..
            }
        ));
        // One end or the other, not neither and not both.
        assert!(Args::try_parse_from([brand::BIN, "lanspeed"]).is_err());
        let both = ["lanspeed", "--serve", "--connect", "192.168.1.20"];
        assert!(Args::try_parse_from([brand::BIN].into_iter().chain(both)).is_err());
        assert!(Args::try_parse_from([brand::BIN, "lanspeed", "--serve", "--port", "7"]).is_ok());
        let wake = ["wake", "aa-bb-cc-dd-ee-ff", "--broadcast", "192.168.1.255"];
        let args = Args::try_parse_from([brand::BIN].into_iter().chain(wake)).expect("a wake");
        assert!(matches!(
            args.mode,
            Mode::Wake { ref macs, port: 9, .. } if macs.len() == 1
        ));
        // A MAC is needed, and the broadcast address is an address.
        assert!(Args::try_parse_from([brand::BIN, "wake"]).is_err());
        let named = ["wake", "aa:bb:cc:dd:ee:ff", "--broadcast", "lan"];
        assert!(Args::try_parse_from([brand::BIN].into_iter().chain(named)).is_err());
    }
}
