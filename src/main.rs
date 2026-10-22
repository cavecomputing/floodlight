//! Entry point: reads the command line, picks the subnet, and opens the search screen (or prints
//! the table when asked for plain output).

mod brand;
mod os;
mod scan;
mod ui;

use clap::Parser;

/// Finds what's on a subnet: every address that answers, with its MAC, maker and name.
#[derive(Parser)]
#[command(name = brand::BIN, version, about)]
struct Args {
    /// The subnet to sweep, like 192.168.1.0/24 (an address alone means its /24).
    /// Defaults to the one this machine is on.
    subnet: Option<String>,
    /// Print the hosts as a table and exit instead of opening the search screen.
    #[arg(long)]
    plain: bool,
    /// Print only the hosts matching this ip, mac, maker or name fragment, then exit.
    #[arg(long, value_name = "QUERY")]
    find: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let subnet = scan::subnet::choose(args.subnet.as_deref())?;
    let runtime = tokio::runtime::Runtime::new()?;
    if args.plain || args.find.is_some() {
        return ui::plain::run(&runtime, subnet, args.find.as_deref());
    }
    ui::app::run(&runtime, subnet, os::settings::load())
}
