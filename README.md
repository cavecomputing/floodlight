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
graphing every round trip and counting the ones lost. And it looks names up in DNS, saying which
server answered, how fast and whether authoritatively, and who registered the domain. And it
measures the throughput between two machines on the LAN, iperf-style, one serving and one
connecting, over TCP or UDP.

Each feature is a mode, typed as a word: `floodlight ping`, `floodlight speedtest`. `floodlight`
on its own lists the modes and the options they share; `floodlight <mode> --help` shows that
mode's own options.

| Mode | What it does |
|---|---|
| [`sweep`](#sweep) | every address on a subnet that answered, with MAC, maker and name |
| [`wireless`](#wireless) | the WiFi networks heard, their channels and how they overlap |
| [`speedtest`](#speedtest) | download, upload, latency and jitter, once or `--count` times |
| [`ping`](#ping) | up to five hosts pinged over and over: every round trip, the ones lost, min, average, max and jitter |
| [`trace`](#trace) | the route to a host, hop by hop, round after round: each router's round trips and the share lost, and where loss starts |
| [`dns`](#dns) | a name's records from the system's name server or any other, timed, authoritative or not, and its whois |
| [`lanspeed`](#lanspeed) | the throughput between this machine and another on the LAN, iperf-style, over TCP or UDP |

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

## trace

Traces the route to a host, the way `traceroute`, `tracert` and `mtr` do: an echo to every hop on
the way, each set to run out (its TTL) at that hop so the router there answers, then again every
second until you quit. Each hop gets a row with its router, the router's name when it has one,
the share of echoes lost and the last, quickest, average and slowest round trip.

```
  # trace example.com (192.0.2.7) · until stopped · 1000 ms apart
  # reached in 3 hops · loss starts at hop 3 · 20.0% lost at the host

  # hop  address              name                 loss    last       min        avg        max
    1    192.168.1.1          router.lan           0.0%    0.6 ms     0.4 ms     0.6 ms     1.1 ms
    2    192.168.2.2          —                    0.0%    9.8 ms     8.9 ms     9.6 ms     12.0 ms
    3    192.0.2.7            example.com          20.0%   timed out  10.1 ms    10.7 ms    13.2 ms
```

```bash
floodlight trace 1.1.1.1                       # until you quit; ^r starts over
floodlight trace example.com --count 30        # thirty rounds, then stop (0 means until you quit)
floodlight trace 1.1.1.1 --wait 2000           # two seconds between rounds (1000 ms by default)
floodlight trace 1.1.1.1 --plain               # ten rounds, then the hops printed (--count for more)
floodlight trace 1.1.1.1 --report trace.pdf    # ten rounds written as a PDF report
```

Where loss starts is the first of the hops that all lose echoes from there to the host. A router
that loses some when the hops after it don't is only slow to answer for itself (routers answer
traffic for others first, and limit how often they answer for themselves), so it isn't traffic
lost and doesn't count. That limit is why `--wait` is a second by default: much faster, and every
router seems to lose echoes. A hop where nothing answered shows `no reply`; many routers never
answer. IPv4 only for now, and no root or Administrator, the same way as `ping`.

## dns

Looks a name up the way `dig` and `nslookup` do and prints the answer: which server was asked and
over what, how long it took, whether the answer was authoritative (from the domain's own name
server) or not (from a resolver, maybe out of its cache), then every record with its TTL. An
address is looked up for its name (a reverse lookup). With no `--server` it asks the name server
this machine uses: `/etc/resolv.conf` on Linux and macOS, the network adapters' settings on
Windows.

```
  # example.com · a · system (192.168.1.1) · udp · 12.4 ms
  non-authoritative · 2 records
  example.com  5m  a  192.0.2.10
  example.com  5m  a  192.0.2.11
```

```bash
floodlight dns example.com                       # its A records, from the system's name server
floodlight dns example.com --type mx,txt         # other record types, several with commas (any works too)
floodlight dns example.com --server 1.1.1.1      # ask another server (an address, address:port or name)
floodlight dns example.com --server 1.1.1.1,8.8.8.8     # each in turn, to compare them
floodlight dns www.example.com --authoritative   # ask the domain's own name server, past any cache
floodlight dns example.com --tcp                 # over TCP (UDP answers cut short retry over TCP anyway)
floodlight dns 192.168.1.20                      # the name an address has (its PTR record)
floodlight dns example.com --whois               # also who registered it: registrar, dates, status, name servers
floodlight dns example.com --plain               # the same without colour
floodlight dns example.com --whois --report dns.pdf     # write it as a PDF report
```

`--whois` asks IANA which registry keeps the domain (or the address), asks that registry, and for
.com and .net the registrar it points to, then shows the fields worth reading. Whois runs over TCP
port 43, which some networks block.

## lanspeed

Measures the throughput between two machines on the LAN, the way iperf does: run it with
`--serve` on one machine, then `--connect` to that machine from the other. Each run sends to the
server for 10 seconds, then receives from it for 10, and the receiving end counts what actually
arrived, reporting a row a second as it goes and a total a direction. Over TCP that is the
throughput the connection reaches. Over UDP it sends at a set rate (`--bitrate`, 10 Mbps unless
told) and also counts the datagrams lost on the way and the jitter (how much one datagram's
transit differs from the one before), so a link that drops under load shows as loss rather than
as TCP quietly slowing down.

```
  # lanspeed to 192.168.1.20 · tcp · 10 s · 4 streams

  # interval      way        bitrate       transfer
    0.0–1.0 s     upload     938.2 Mbps    117.3 MB
    1.0–2.0 s     upload     941.0 Mbps    117.6 MB
    ...
    10.0 s total  upload     939.8 Mbps    1.17 GB
    0.0–1.0 s     download   936.4 Mbps    117.1 MB
```

```bash
floodlight lanspeed --serve                              # on one machine: wait for a client, until you quit
floodlight lanspeed --connect 192.168.1.20               # on the other: the screen, one run; ^r runs it again
floodlight lanspeed --connect 192.168.1.20 --udp --bitrate 500    # UDP at 500 Mbps: loss and jitter too
floodlight lanspeed --connect 192.168.1.20 --streams 4   # four connections at once, when one won't fill the link
floodlight lanspeed --connect 192.168.1.20 --time 30 --interval 5    # 30 s each way, a row every 5 s
floodlight lanspeed --connect 192.168.1.20 --download    # one way only (--upload is the other)
floodlight lanspeed --connect 192.168.1.20 --count 3 --wait 60       # three runs, a minute apart
floodlight lanspeed --serve --port 5300                  # another port (5201, iperf's, by default; same on both ends)
floodlight lanspeed --connect 192.168.1.20 --plain       # print each row as it arrives
floodlight lanspeed --connect 192.168.1.20 --report lan.pdf    # write it as a PDF report
```

The server prints a line a test and takes one at a time; a second client is told it is busy. It
talks only to another copy of Floodlight, not to iperf, since the two don't speak the same
protocol. UDP at `--bitrate 0` sends as fast as the machine can, which floods most links and shows
up as loss; a rate just under what TCP reached is the useful test. The server's port has to be
open on its firewall, TCP and UDP both.

## --report

Add `--report FILE` to `sweep`, `wireless`, `speedtest`, `ping`, `dns` or `lanspeed` to run it, write it to `FILE` as
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
- The LAN speed report leads with the throughput each way (the median across runs), the jitter
  and loss for UDP, a summary of what the link carried, a chart a direction (of each interval for
  one run, of each run's total for several), and every run in a table.
- The DNS report leads with the name, the server and how the answer went, says what an
  authoritative or non-authoritative answer means, then has a table of records for each lookup and
  the whois fields when asked for.

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
