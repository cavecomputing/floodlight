<p align="center">
  <img src="assets/logo.svg" alt="Floodlight" width="128">
</p>

<p align="center">
  Shine a light on the network around you, from the terminal.
</p>


Floodlight is a small terminal tool that looks at the network this machine is on. It finds every
device on your subnet, with its MAC, maker and name, so the camera or switch you just plugged in
turns up by the MAC on its label. And it lists the WiFi networks around you and how their channels
crowd each other, so you can pick the quietest one.

Each feature has its own flag. `floodlight` on its own prints the help that lists them.

| Flag | What it does |
|---|---|
| [`--sweep`](#--sweep) | every address on a subnet that answered, with MAC, maker and name |
| [`--wireless`](#--wireless) | the WiFi networks heard, their channels and how they overlap |
| [`--report FILE`](#--report) | either of them written as a PDF to hand a client or a manager |

It needs no root, uses the tools your OS already has, and runs on Linux, macOS and Windows.

## Quick start

All you need is [Rust](https://rustup.rs) and a C compiler (`base-devel` on Arch,
`build-essential` on Ubuntu, Xcode's command line tools on macOS, Visual Studio Build Tools on
Windows).

```bash
cargo install --git https://github.com/cavecomputing/floodlight
floodlight --sweep
```

`cargo install` builds the release binary and puts it in `~/.cargo/bin`, which rustup already
added to your `PATH`. Run the same command again to update.

## --sweep

Sweeps a subnet and shows everything that answered: address, MAC, maker and name. Type part of
any of them to find the one you want. Made for the moment a camera or switch has just been
plugged in and all you have is the MAC on its label.

```
  # 192.168.1.0/24 · en0 · 254 addresses
  > 3d:57

  # ip               mac                maker                name                ping
    192.168.1.108    2c:a5:9c:0b:3d:57  Hangzhou Hikvision   doorcam.local       ● 1.2 ms
```

```bash
floodlight --sweep                  # the subnet this machine is on
floodlight --sweep 192.168.1.0/24   # a subnet, or an address for its /24
floodlight --sweep --plain          # print the table and exit
floodlight --sweep --find 3d:57     # print only the matching rows
floodlight --sweep --report lan.pdf # write it as a PDF report (see --report)
```

On screen: type to filter (an address fragment, a MAC with or without separators, a maker, a
name), ↑↓ to move, **enter** copies the address, **tab** copies the MAC, **^r** sweeps again,
**esc** clears, **^c** quits. The colours are your terminal's own.

Needs no root: hosts are found with the system `ping` and the OS's ARP table, names with mDNS and
reverse DNS, makers from the IEEE registry built into the binary. Linux, macOS and Windows.

## --wireless

Lists every WiFi network this machine can hear, with its channel, width and signal, how many
neighbours share its channel (they take turns) or overlap it from a nearby one (they talk over
each other, which is worse), and a strip of its band so the rows stacked together draw the
overlap. The top line names the quietest channel in each band.

```
  # 8 networks · quietest 2.4 GHz 11 · 5 GHz 44

  # ssid          ch   band     width    signal   trend          neighbours          air
    Home · joined 6    2.4 GHz  —        -59 dBm  +2 · peak -57  1 same · 1 overlap  ···──●──·····
    Neighbour     6    2.4 GHz  —        -80 dBm  ±0 · peak -78  1 same · 1 overlap  ···──●──·····
    Cafe Guest    8    2.4 GHz  —        -73 dBm  new            3 overlap           ·····──●──···
```

```bash
floodlight --wireless           # the screen, scanning until you quit; ^r starts over
floodlight --wireless --plain   # print one scan's table and exit
floodlight --wireless --report wifi.pdf   # write it as a PDF report
```

The screen keeps scanning, a second after each scan is in, so you can walk around with it. Every
access point heard stays on the list in a steady order (dimmed as `gone` while out of range), and
a trend column shows the dBm each gained or lost since the scan before and the strongest it has
been. Walk towards a rogue access point and its number climbs. **tab** cycles the order: by channel, loudest
first, then by name (access points sharing a name by band, then loudest first). Signals are ranked when you
switch order and then hold still (anything new joins the bottom); cycle round to re-rank. **^r** forgets it all and starts over,
for a new spot. How fresh each scan is depends on the OS: iwd and macOS scan every time; NetworkManager
and Windows may refuse a new scan within a few seconds of the last and repeat it.

Read from the OS's own tool, no root: `nmcli` (NetworkManager) or `iw` on Linux (with iwd, as on
Omarchy, install `iw`; iwd is asked for a fresh scan first), Windows' own wifi service on Windows. macOS hides
network names from terminal tools, so there the list comes from a small helper app built in;
the first run asks to allow it Location (it never reads where you are, it's just the permission
macOS ties names to). If you said no, allow `Floodlight WiFi` under System Settings › Privacy &
Security › Location Services. A width the OS doesn't report (nmcli) shows `—` and counts
as 20 MHz. Recent Windows may refuse to list networks until the terminal has location access.

## --report

Add `--report FILE` to `--sweep` or `--wireless` to run one sweep or survey, write it to `FILE` as
a PDF, and exit. The report is a Letter page or more, laid out to hand to a client or a manager:
a header with the date and the key numbers, a summary in plain sentences, then the table.

- The sweep's report: the subnet, how many addresses answered, how many devices have a known
  maker or a randomized MAC, and every device's address, MAC, maker, name and ping. `--find`
  narrows the table the same way it narrows `--plain`.
- The survey's report scans for 10 seconds first (`--scan-for SECONDS` changes that) and keeps
  every network heard in any scan, since weak ones often answer only the second or third time. It
  shows the network this machine is on and how crowded its channel is, the quietest channel in
  each band, a chart of each band (a lane per network over the slice of spectrum it fills, the
  quietest channel marked), and every network in a table.

The PDF uses the fonts every PDF reader has built in, so it needs nothing installed and opens
anywhere.

## Building from a clone

```bash
git clone https://github.com/cavecomputing/floodlight.git
cd floodlight
cargo run -- --sweep        # a debug build, straight from the source
cargo install --path .      # or install it, like the quick start does
```

On Linux, copying a row needs `wl-copy` (wl-clipboard), `xclip` or `xsel`.

## Licence

MIT. The maker registry in `assets/oui.csv` is the IEEE's public MA-L list.
