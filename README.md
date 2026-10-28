<p align="center">
  <img src="assets/logo.svg" alt="Floodlight" width="128">
</p>

<p align="center">
  Shine a light on the network around you, from the terminal.
</p>


Floodlight is a small terminal tool that looks at the network this machine is on. It finds every
device on your subnet, with its MAC, maker and name, so the camera or switch you just plugged in
turns up by the MAC on its label. And it lists the WiFi networks around you and how their channels
crowd each other, so you can pick the quietest one. And it tests the internet connection's
download, upload, latency and jitter against Cloudflare. And it pings up to five hosts over and over,
graphing every round trip and counting the ones lost.

Each feature is a mode, typed as a word: `floodlight ping`, `floodlight speedtest`. `floodlight`
on its own lists the modes and the options they share; `floodlight <mode> --help` shows that
mode's own options.

| Mode | What it does |
|---|---|
| [`sweep`](#sweep) | every address on a subnet that answered, with MAC, maker and name |
| [`wireless`](#wireless) | the WiFi networks heard, their channels and how they overlap |
| [`speedtest`](#speedtest) | download, upload, latency and jitter, once or `--count` times |
| [`ping`](#ping) | up to five hosts pinged over and over: every round trip, the ones lost, min, average, max and jitter |

| Shared option | What it does |
|---|---|
| `--plain` | print the table and exit instead of opening the screen |
| [`--report FILE`](#--report) | write the result as a PDF to hand a client or a manager, then exit |

```bash
floodlight                    # the modes and the shared options
floodlight ping --help        # ping's own options: the hosts, --count, --wait
```

The help is in colour in a terminal and plain when piped or when `NO_COLOR` is set.

It needs no root, uses the tools your OS already has, and runs on Linux, macOS and Windows.

## Quick start

All you need is [Rust](https://rustup.rs) and a C compiler (`base-devel` on Arch,
`build-essential` on Ubuntu, Xcode's command line tools on macOS, Visual Studio Build Tools on
Windows).

```bash
cargo install --git https://github.com/cavecomputing/floodlight
floodlight sweep
```

`cargo install` builds the release binary and puts it in `~/.cargo/bin`, which rustup already
added to your `PATH`. Run the same command again to update.

## sweep

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
floodlight sweep                    # the subnet this machine is on
floodlight sweep 192.168.1.0/24     # a subnet, or an address for its /24
floodlight sweep --plain            # print the table and exit
floodlight sweep --find 3d:57       # print only the matching rows
floodlight sweep --report lan.pdf   # write it as a PDF report (see --report)
```

On screen: type to filter (an address fragment, a MAC with or without separators, a maker, a
name), ↑↓ to move, **enter** copies the address, **tab** copies the MAC, **^r** sweeps again,
**esc** clears, **^c** quits. The colours are your terminal's own.

Needs no root: hosts are found with the system `ping` and the OS's ARP table, names with mDNS and
reverse DNS, makers from the IEEE registry built into the binary. Linux, macOS and Windows.

## wireless

Lists every WiFi network this machine can hear, with its channel, width and signal, and a strip
of its band so the rows stacked together draw who overlaps whom. The top line names the quietest channel in each band.

```
  # 8 networks · quietest 2.4 GHz 11 · 5 GHz 44

  # ssid          ch   band     width    signal   trend          air
    Home · joined 6    2.4 GHz  —        -59 dBm  +2 · peak -57  ···──●──·····
    Neighbour     6    2.4 GHz  —        -80 dBm  ±0 · peak -78  ···──●──·····
    Cafe Guest    8    2.4 GHz  —        -73 dBm  new            ·····──●──···
```

```bash
floodlight wireless             # the screen, scanning until you quit; ^r starts over
floodlight wireless --plain     # print one scan's table and exit
floodlight wireless --report wifi.pdf     # write it as a PDF report
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

## speedtest

Tests the internet connection against [Cloudflare's speed test](https://speed.cloudflare.com), the
way its own page does: 20 small requests for latency (the middle one) and jitter (how much one
differs from the next), then downloads and uploads taking turns, from 100 kB up to 250 MB, until
every request of a size takes over a second. From 10 MB up, each transfer is split over four
connections at once, the way fast.com and Ookla measure, since one connection rarely fills a fast
line; a run still moves no more data than Cloudflare's own page. The speed is the 90th percentile of those transfers, as Cloudflare reports it, so the small
ones that never fill the link don't drag it down.

```
  # cloudflare LAX · Los Angeles · Example Fiber · median 240.1 Mbps down · 40.2 Mbps up

  # time      latency    jitter     download      upload        server
    14:05:09  12.1 ms    0.8 ms     241.3 Mbps    40.2 Mbps     LAX · Los Angeles
    14:05:34  11.9 ms    1.1 ms     238.7 Mbps    40.5 Mbps     LAX · Los Angeles
```

```bash
floodlight speedtest                       # the screen, one run; ^r runs it again
floodlight speedtest --count 6             # six runs, 5 seconds apart
floodlight speedtest --count 6 --wait 60   # six runs, a minute apart
floodlight speedtest --plain               # print each run's row as it ends
floodlight speedtest --count 3 --report speed.pdf     # write them as a PDF report
```

With `--count`, the screen also graphs download, upload and latency across the runs, side by side
under the table, so a line that slows down over the afternoon shows as a falling row of dots.

It talks to Cloudflare over HTTPS by itself, so nothing needs installing. A test moves a few
hundred megabytes on a fast line.

## ping

Pings up to five hosts side by side, over and over, and shows every reply as it comes back, the
ones lost (and why: `timed out`, `unreachable`), and each host's totals so far: sent, lost, and the
minimum, average and maximum round trip with the jitter (how much one differs from the next).
Each host gets a graph along the bottom that plots its round trips, a gap where one was lost, so a
link that drops out or slows down shows at a glance, and which hop it is shows by which graph has
the gaps. With no host it pings this machine's gateway (its router).

```
  # ping 2 hosts · until stopped · 500 ms apart
  # gateway (192.168.1.1) · 42 sent · 1 lost (2.4%) · min 0.61 · avg 1.12 · max 4.80 · jitter 0.35 ms
  # 1.1.1.1               · 42 sent · 0 lost (0.0%) · min 9.80 · avg 11.02 · max 14.10 · jitter 0.90 ms

  # seq     time      host                   latency
    41      14:05:30  gateway (192.168.1.1)  timed out
    41      14:05:30  1.1.1.1                10.85 ms
    42      14:05:32  gateway (192.168.1.1)  1.04 ms
```

```bash
floodlight ping                         # the gateway, until you quit; ^r starts over
floodlight ping 1.1.1.1                 # a host by address or by name
floodlight ping 192.168.1.1 1.1.1.1     # up to five hosts at once, a graph each
floodlight ping 1.1.1.1 --count 100     # a hundred pings, then stop (0 means until you quit)
floodlight ping --wait 1000             # a second between pings (500 ms by default)
floodlight ping 1.1.1.1 --count 20 --plain         # print each ping as it comes back, then the totals
floodlight ping 1.1.1.1 --count 50 --report ping.pdf     # write them as a PDF report
```

`--wait` is in milliseconds here (in seconds for `speedtest`), counted from one reply, or a
reply given up on after 2 seconds, to the next ping. Each round trip is timed to a fraction of a
millisecond on every OS (Windows' own `ping` only counts whole ones). IPv4 only for now.

No root and no Administrator: it uses the ping socket Linux and macOS give ordinary programs and
Windows' own ping call. Most Linux systems allow that socket out of the box (systemd turns it on);
if yours doesn't, Floodlight says so and gives the one `sysctl` line that allows it.

## --report

Add `--report FILE` to `sweep`, `wireless`, `speedtest` or `ping` to run it, write it to `FILE` as
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
- The speed test's report runs `--count` times (once unless told) and leads with the median
  download and upload, then the latency, jitter, the Cloudflare data center and the internet
  provider, a summary of the range across runs, a chart each of download, upload and latency
  across the runs (when there are several) to show the trend, and every run in a table.
- The ping report pings each host `--count` times (10 unless told; it needs an end, so not 0)
  and leads with the share lost (and for one host the average, minimum, maximum and jitter), a
  summary that names the pings lost, a table of each host's totals when there are several, and a
  chart of each host's round trips. Every ping is in the charts, so there is no table of them.

The PDF uses the fonts every PDF reader has built in, so it needs nothing installed and opens
anywhere.

## Building from a clone

```bash
git clone https://github.com/cavecomputing/floodlight.git
cd floodlight
cargo run -- sweep          # a debug build, straight from the source
cargo install --path .      # or install it, like the quick start does
```

On Linux, copying a row needs `wl-copy` (wl-clipboard), `xclip` or `xsel`.

## Licence

MIT. The maker registry in `assets/oui.csv` is the IEEE's public MA-L list.
