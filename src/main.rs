//! Entry point: reads the command line, picks the feature, and opens its screen (or prints its
//! table when asked for plain output).

mod brand;
mod os;
mod scan;
mod ui;
mod wifi;

use clap::{ArgGroup, Parser};

/// Looks at the network around this machine. Pick a feature with its flag.
#[derive(Parser)]
#[command(name = brand::BIN, version, about, arg_required_else_help = true)]
#[command(group(ArgGroup::new("feature").required(true).args(["sweep", "wireless"])))]
struct Args {
    /// Sweep a subnet: every address that answers, with its MAC, maker and name.
    #[arg(long)]
    sweep: bool,
    /// Survey the WiFi here: every network heard, how the channels overlap, the quietest one.
    #[arg(long)]
    wireless: bool,
    /// The subnet to sweep, like 192.168.1.0/24 (an address alone means its /24).
    /// Defaults to the one this machine is on.
    #[arg(requires = "sweep")]
    subnet: Option<String>,
    /// Print the table and exit instead of opening the screen.
    #[arg(long)]
    plain: bool,
    /// Print only the hosts matching this ip, mac, maker or name fragment, then exit.
    #[arg(long, value_name = "QUERY", requires = "sweep")]
    find: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = tokio::runtime::Runtime::new()?;
    if args.wireless {
        if args.plain {
            return ui::plain::wireless(&runtime);
        }
        return ui::wireless::run(&runtime, os::settings::load());
    }
    let subnet = scan::subnet::choose(args.subnet.as_deref())?;
    if args.plain || args.find.is_some() {
        return ui::plain::run(&runtime, subnet, args.find.as_deref());
    }
    ui::app::run(&runtime, subnet, os::settings::load())
}
