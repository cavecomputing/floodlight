# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

The app is **Floodlight**, a terminal tool written in Rust. Each feature is a mode, typed as a word. `sweep`
sweeps a subnet and shows every address that answered with its MAC, maker and name, under a prompt
that filters the rows as you type, so a camera or switch you just plugged in is found by the MAC
on its label (or the other way round). `wireless` lists the WiFi networks in earshot and how
their channels overlap. `speedtest` measures the internet connection's download, upload, latency
and jitter against Cloudflare. `ping` pings up to five hosts (the gateway by default) over and over,
graphing every round trip and counting the ones lost. `dns` looks a name up like dig, from the
system's name server or one typed, saying how fast and whether authoritatively, and `--whois` who
registered it. `lanspeed` measures the throughput between two machines on the LAN, iperf-style:
`--serve` on one, `--connect` from the other, over TCP or UDP. `wake` sends a Wake-on-LAN packet
to a MAC. Linux (the owner's Omarchy/Hyprland machines) comes first, and it also builds and runs on
macOS and Windows: a change for one OS must leave the others working, Linux above all.

The owner is new to Rust. When you report back, explain Rust-specific things in plain words (what a
crate is, why the borrow checker complained, what a command does) rather than assuming he knows
them. [Rust in plain terms](#rust-in-plain-terms) below is the shared vocabulary.

## This repository is public

Nothing personal goes in: no names, email addresses, chat handles, real subnets, real MAC
addresses or hostnames, in code, tests, docs or commit messages. Tests use `192.168.1.x` and
made-up MACs. The brand name cavecomputing (whose design system the look follows) is the only
proper noun that belongs here. The IEEE registry in `assets/oui.csv` is public data.

## Git workflow

Same rule as the owner's other solo projects: **commit and push to `main` as needed**, without
asking. There are no outside users yet, and the owner wants the work to land where he can see it.

- Commit when a piece of work is finished and verified (see [Verification](#how-much-verification-a-change-needs)),
  then `git push`. Don't leave finished work sitting only in the working tree.
- **One commit, one concern.** A commit should be reviewable and revertable on its own, so split
  unrelated work. Size is not the test; twenty files for one concern is still one commit.
- Say in the message what broke and why the fix works, not just what you typed.
- Never commit a red build. `cargo build`, `cargo test` and `cargo clippy` must all pass first.
- Only include files that belong to the task.

[AGENTS.md](AGENTS.md) is a short pointer to this file, not a copy. Edit this file directly.

## No hardcoded app name

The name, the command and the config folder all come from **[src/brand.rs](src/brand.rs)**.
Renaming the tool again must mean editing that one file.

- Never write "Floodlight" or `floodlight` anywhere else in `src/`. Use the constants. A grep for
  it outside `brand.rs` should find only docs.
- The binary name in `Cargo.toml` is the one unavoidable exception; it is noted next to the
  constants in `brand.rs` so a rename touches both.

## Least code that does the job

Aim for the smallest diff that completes the task in full. The app stays approachable for someone
learning Rust only while the code stays small and plain.

- Prefer editing existing code over adding new code, and a few lines at the right call site over a
  new helper, module or trait. Add a module when a concern genuinely needs its own home.
- Reuse what is already here: the palette roles in `ui/theme.rs`, the MAC normaliser and the query
  matcher in `scan/hosts.rs`, the constants in `brand.rs`. A second implementation of something
  the repo already does is the most expensive kind of code.
- **No new crate without a reason you can state in one line**, written as a comment above it in
  `Cargo.toml` (every dependency there has one). When a crate is faster or works on more systems
  than shelling out to the OS's own tool, use the crate.
- Leave out what nothing needs yet: config knobs, generic traits with one implementation,
  `Box<dyn>` where a plain enum works, error handling around code that cannot fail.
- Plain Rust over clever Rust. Prefer a `match` to a chain of combinators when it reads better,
  owned `String`s over lifetimes when performance doesn't care, and no macros of our own.
- Deleting code is a legitimate way to finish a task.

This governs the amount of code, not the amount of work. Deliver everything asked for.

## Naming

A name is the cheapest documentation. Make it a concise nameplate for what the thing is or does.

- Prefer the specific noun to the category: `host`, `query`, `vendor` over `data`, `value`, `item`.
- Name a function for what it returns or does: `normalize_mac()`, `shows()`, `sweep_again()`.
- Rust's conventions are not optional: `snake_case` for functions, variables and modules,
  `CamelCase` for types and enum variants, `SCREAMING_SNAKE_CASE` for constants. `cargo clippy`
  will complain otherwise.
- Don't abbreviate past recognition.

Renaming existing code is its own task. Don't fold a rename into an unrelated change.

## Rust in plain terms

- **Crate**: a Rust package. Our app is one crate; the libraries it uses (ratatui, tokio, ...) are
  crates too, downloaded from crates.io.
- **Cargo**: Rust's build tool and package manager in one. It reads `Cargo.toml` (the list of
  dependencies and settings) and `Cargo.lock` (the exact versions it picked, committed so every
  build is the same).
- **Module**: one `.rs` file (or folder) inside the crate. `mod scan;` in `main.rs` pulls in
  `src/scan/mod.rs`, which lists the files in that folder.
- **`target/`**: where compiled output goes. It is large, ignored by git, and safe to delete
  (`cargo clean`); the next build just takes longer.
- The first build downloads and compiles every dependency, which takes a minute or two. Later
  builds only recompile what changed.
- **Debug vs release**: `cargo run` builds a debug binary, quick to compile but slower to run.
  `cargo build --release` takes longer and gives the fast, small binary you'd actually install.
- **`Result` and `?`**: a function that can fail returns `Result<T, E>`; `?` means "if this failed,
  return the error to my caller". `Option<T>` is a value that may be missing, and `?` works on it
  the same way inside a function that returns an `Option`.
- **`unwrap()` / `expect()`**: crash if the value is an error or missing. Fine in tests; in app code
  only where failure is genuinely impossible, with `expect("why this can't fail")`.
- **async / await and tokio**: an `async fn` is a job that can pause while it waits (for a ping to
  come back, say) so other jobs run in the meantime. tokio is the runtime that runs those jobs on
  a few threads. `tokio::spawn` starts one in the background; `.await` waits for one to finish.
  The sweep is async so that 128 pings can be out at once; the screen is ordinary code that just
  checks a channel for what the sweep found.
- **Channel** (`mpsc`): a queue between two parts of the program. The sweep sends `Event`s into
  it; the screen takes them out when it next redraws.
- **`#[cfg(...)]` and `cfg!(...)`**: compile the next item only on some platforms, or test the
  platform inside a function (both branches still have to compile). Code behind a cfg for another
  OS isn't even compiled on this one, which is why the OS-specific commands are chosen with `cfg!`.
- **Target**: the OS and processor a build is for. Cargo builds for the machine it runs on unless
  given `--target`.

## Build / run / test

```bash
cargo run                              # build (debug) and print the help: the modes and the shared options
cargo run -- ping --help               # one mode's own options
cargo run -- sweep                     # sweep the subnet this machine is on
cargo run -- sweep 192.168.1.0/24      # sweep that subnet (everything after -- goes to the app)
cargo run -- sweep --plain             # print the table and exit (for pipes, scripts, a quick look)
cargo run -- sweep --filter 3d:57,192.168.1.10-20    # open with that typed at the prompt: rows matching any
                                                    # address, range, subnet, or mac, maker or name fragment
cargo run -- sweep --filter 3d:57 --plain           # print only the matching rows
cargo run -- wireless                  # the WiFi networks heard here and how their channels overlap
cargo run -- wireless --plain          # the same as a table on stdout
cargo run -- speedtest                 # download, upload, latency and jitter against Cloudflare
cargo run -- speedtest --count 3 --wait 10 --plain    # three runs, 10 s apart, printed
cargo run -- ping                      # ping the gateway until you quit (--count 0, the default)
cargo run -- ping 1.1.1.1 --count 20 --wait 250 --plain    # twenty pings, 250 ms apart, printed
cargo run -- dns example.com --type a,mx --server 1.1.1.1    # records, timed, authoritative or not
cargo run -- dns example.com --whois --report out.pdf        # with who registered it, as a PDF
cargo run -- lanspeed --serve                                # wait for a client on port 5201, tcp and udp
cargo run -- lanspeed --connect 192.168.1.20 --udp --bitrate 500 --streams 4 --time 30 --plain
cargo run -- wake aa:bb:cc:dd:ee:ff --broadcast 192.168.1.255    # the magic packet, one line, exit
cargo run -- sweep --report out.pdf    # one sweep written as a PDF report, then exit (wireless too)
cargo run -- wireless --report out.pdf --scan-for 20    # scan 20 s before writing (default 10)

cargo test                        # run every test
cargo test mac                    # run only tests whose name contains "mac"
cargo clippy --all-targets        # the linter: catches common mistakes and non-idiomatic code
cargo fmt                         # reformat all code to the standard style (4-space indents)

cargo build --release             # the optimised binary, at target/release/<bin name> (.exe on Windows)
```

What each OS needs before the first build: Rust (rustup) and a C compiler (`base-devel` on Arch,
`build-essential` on Ubuntu, Xcode's command line tools on macOS, Visual Studio Build Tools with
the "Desktop development with C++" workload on Windows). At run time: `ping`, and `ip` (iproute2)
or `arp`, which every OS has; on Linux `wl-copy` (wl-clipboard), `xclip` or `xsel` for copying.
For `wireless`: `nmcli` or `iw` on Linux, nothing extra on Windows (it asks the wifi service through windows-sys); on macOS the build compiles a
Swift helper with `swiftc` (part of Xcode's command line tools).

Installing for daily use: `cargo install --path .` puts the binary in `~/.cargo/bin`.

Releasing: set `version` in `Cargo.toml`, commit, then `git tag v0.1.0 && git push origin v0.1.0`.
`.github/workflows/release.yml` checks the tag matches that version, runs the tests on Windows,
builds the release binary and publishes a GitHub release with the `.exe` attached. A tag with a
suffix (`v0.2.0-rc.1`) becomes a pre-release.

### How much verification a change needs

- Every change: `cargo build`, `cargo test` and `cargo clippy --all-targets` with no warnings,
  and `cargo fmt --check` clean.
- Code that only one OS needs goes behind `#[cfg(...)]` or `cfg!(...)`, with a comment saying what
  the other OSes do. The ARP and WiFi parsers have a test per OS format so the others can't
  break quietly.
- Logic (matching, MAC normalising, the ARP and ping parsers, the mDNS packet, the WiFi parsers
  and channel maths): unit tests next to the code, in a `#[cfg(test)] mod tests` block at
  the bottom of the file. Keep that logic out of `ui/` so it can be tested without a terminal.
- Anything visual (layout, colours, the status bar): run the app in a terminal and look, under a dark
  and a light terminal theme. `--plain` checks the sweep itself without a screen. Never claim a change was
  checked on screen when it wasn't.
- The sweep touches a real network. Run it on the LAN you are on; don't put its output (real
  addresses and MACs) in the repo.

## Architecture

Each feature is a mode, a clap subcommand typed as a plain word (`floodlight ping`), and the
command alone prints the help listing them. `floodlight ping --help` shows only that mode's
options; `--plain` and `--report` are global, shared by every mode. The help's colours are `HELP` in `ui/theme.rs` (clap drops them when piped).

`sweep`: `floodlight sweep [subnet]` opens the screen, starts the sweep in the
background, and fills the table as answers come in. Ctrl+R sweeps again.

`--report FILE`: with either feature, runs it once, writes `FILE` as a PDF and exits. The PDF is
written by hand in `report/` (no crate): the built-in Helvetica and Courier, which every reader
has, so nothing is embedded. It follows the design system's print documents (the light theme on a
cream Letter page, yellow edges, small-capital headings, rounded tables) with **no branding**: no
name or logo anywhere on it.

`wireless`: `floodlight wireless` opens a screen of every network heard and keeps asking the
OS's own tool for the list in the background, a second after each answer, keeping the rows up
meanwhile; `wifi/trend.rs` keeps every access point heard and tracks its signal across scans.
Tab cycles channel, signal (ranked once when chosen, so rows hold still) and name order;
Ctrl+R forgets them and starts over. `--plain` prints one scan.

`speedtest`: `floodlight speedtest` opens a screen with a row a run and fills it in as the run
goes: latency and jitter from 20 empty requests, then Cloudflare's own page's sequence of
downloads and uploads taking turns as they grow (100 kB up to 250 MB), each direction stopping once
every request of a size takes over a second, reporting the 90th percentile. From 10 MB up a
transfer is split over four connections at once (a thread each, a quarter of the size each, the
slowest one's time), since one connection and one thread's TLS top out well under a gigabit; the
page's own single stream does too, which is why it reads low against fast.com on a fast line.
Splitting keeps a run's data to what the page moves: four times that set off Cloudflare's limit.
The four connections are opened untimed before the first split transfer and kept open between
them (ureq keeps only 3 idle a host unless told), so no transfer times a TLS setup. The method
follows the npm package `@cloudflare/speedtest` (the page's engine): every request carries the
page's `referer` (other clients were limited without it), the server's time from each answer's
`server-timing` header comes off downloads and pings, uploads post the text `0` to
`__up?bytes=N` and are timed to the answer, 429s are waited out and retried when they ask for
10 s or less. Cloudflare answers 429 with `retry-after: 3600` once a machine has run many tests in
a short while; that fails the run, saying so, and no further runs start, since the limit holds for
the machine. A 403 (Cloudflare's firewall, seen partway through real runs) isn't asked again, as in
Cloudflare's own engine; it leaves that direction with what it measured, and the run fails only
when it had measured nothing. A failed run's partial numbers stay in its row but out of the
medians. It speaks HTTPS through the ureq crate on a thread of its own, connections kept open,
cookies kept, no gzip. Cloudflare publishes no limits or terms for this; they have changed
without notice (the server-timing format, the referer), so check its own engine when it breaks.
`--count N` runs it N times, `--wait SECONDS` apart (5 by default); Ctrl+R runs the same again once it is done.

`ping [HOST]...`: pings up to five hosts (names or IPv4 addresses; the gateway from the OS's
route table when none is given), each on a thread of its own with its own socket, all sending
their `Ping`s (tagged with the host's index) into one channel, one echo at a time: up to 2 s for the reply, then `--wait`
**milliseconds** (500 by default; the same flag is seconds for `speedtest`) before the next.
`--count N` stops after N; 0, the default, pings until you quit, and `--report` (which needs an
end) takes 10 unless told. No root and no Administrator: Linux and macOS send through an ICMP
datagram socket (socket2), which Linux allows when `net.ipv4.ping_group_range` covers the user
(systemd sets it so); Windows uses its own `IcmpSendEcho` (windows-sys). Every round trip is
timed with `Instant` around the send and the reply, since Windows only reports whole
milliseconds. Ctrl+R forgets the pings and starts over. The screen and the report give each host
a graph of its own (three to a row on screen); the report has no table of every ping.

`dns NAME`: asks for the `--type`s (A, or PTR for an address, by default) from each `--server`
(the first one in `/etc/resolv.conf` on Linux and macOS, `GetNetworkParams` on Windows), one
question at a time over UDP, or TCP with `--tcp` or when the UDP answer comes back truncated. The
messages are built and read by hickory-proto; the sockets are std's own, timed with `Instant`.
The AA flag says authoritative or not; `--authoritative` finds the zone's first NS (walking up the
name) through the usual server and asks it instead. Its answer is printed and stays in the
terminal like dig's, with no screen: styled lines (`ui/dns.rs`) written through crossterm, plain
under `--plain`, a pipe or `NO_COLOR`. `--whois` asks whois.iana.org for the registry (`refer:`),
the registry for the name (dropping labels from the left until it knows one), and the registrar
it names, and keeps the fields in `FIELDS` (`dns/whois.rs`).

`lanspeed`: `floodlight lanspeed --serve` listens on one port (5201, iperf's, so the same
firewall rule serves), TCP and UDP both, takes one test at a time (a second client hears `busy`)
and prints a line a test; `--connect HOST` runs a test against it: a transfer each way in turn
(upload, then download; `--upload` or `--download` for one), each for `--time` seconds over
`--streams` connections (or UDP sockets) at once, the receiving end reporting what arrived every
`--interval` seconds and a total. The two ends speak their own line protocol (`lanspeed/wire.rs`:
`hello key=value ...`, `ok token=`, `data token=` on each stream, `interval`/`total` lines back to
the client, `done`), not iperf's, so it only talks to another copy of this tool. A UDP datagram
carries a sequence number and the sender's clock, so the receiver counts the ones lost by the gaps
and the jitter as RTP does (RFC 3550, smoothed by a sixteenth), per stream; `--bitrate` paces the
sender (0 is unpaced, a flood). The receiving socket's buffer is widened through socket2, since
the OS's default drops datagrams at a few hundred Mbps. Everything is std sockets and threads,
timed with `Instant`; `--count` and `--wait` repeat runs like `speedtest`. Checked over loopback
only, so far.

`wake MAC...`: reads every MAC first (through `normalize_mac`, so any format the sweep reads),
then broadcasts each its magic packet (six `0xff` bytes, the MAC sixteen times) over UDP to
`--broadcast` (255.255.255.255 unless told) on `--port` (9), prints a line a MAC and exits.
Nothing answers a wake packet, so the line says it was sent, not that the machine woke.

There is no daemon and no state between runs.

The screen is [ratatui](https://ratatui.rs): every tick (50ms) it drains the sweep's channel into
`Hosts`, redraws, and handles one key press. The sweep runs on a tokio runtime on other threads
and only ever talks to the screen through `Event`s.

### Folder layout

The code is split by *what it knows about*. The rule that keeps it organised: **`scan/`, `wifi/`,
`speedtest/`, `ping/`, `dns/`, `lanspeed/` and `os/` never import ratatui**, so all the logic runs in tests, and `ui/` holds only drawing and
key wiring.

```
Cargo.toml, Cargo.lock   dependencies, each with its one-line reason (see "Rust in plain terms")
build.rs                 runs before the build: on macOS, compiles src/wifi/helper.swift
assets/oui.csv           the IEEE MA-L registry, cut to prefix and name; assets/README.md says how to refresh it
assets/logo.svg          the logo the README shows: a lamp lighting the devices below it, in the Gruvbox yellow
src/
  main.rs                entry point: reads the command line, picks the subnet, opens the screen or prints
  brand.rs               every name (see above)
  scan/                  the sweep, no UI
    subnet.rs            the subnet to sweep: typed, or found from this machine's interfaces
    sweep.rs             the sweep itself: pings at once, the ARP table on a timer, makers and
                         names filled in; the Event enum the screen hears
    ping.rs              one address: the UDP poke and the system ping (IcmpSendEcho on Windows), and reading its round trip
    arp.rs               the OS's ARP table in its three formats (arp -an, ip neigh, Windows arp -a),
                         and whether Linux has heard from each entry lately
    mdns.rs              a reverse lookup over mDNS: the PTR question packet and reading the reply
    vendor.rs            the maker from a MAC's first three bytes; randomized addresses; tidied names
    hosts.rs             a Host row, the Hosts table built from Events, MAC normalising, and how a
                         Query matches a row (address, MAC with or without separators, maker, name)
  wifi/                  the WiFi survey, no UI
    survey.rs            the networks heard, from nmcli / iw / the macOS helper, a parser each, and
                         the width and bonded centre read from a beacon's operation elements
    windows.rs           Windows' wifi service (wlanapi) asked directly: scan, the list heard, the joined one
    helper.swift         the macOS helper app: names need Location, which only an app can be given
    channels.rs          channels as frequencies: bands, the slice a network fills, who shares or
                         overlaps it, the quietest channel, the strip that draws a band
    trend.rs             each access point's signal across scans: change and peak, gone or heard
  speedtest/             the speed test, no UI
    cloudflare.rs        speed.cloudflare.com through ureq: the transfers and their timings, the data center
    runs.rs              the runs: the sizes, the Event enum the screen hears, latency, jitter and
                         the 90th percentile speeds
  ping/                  pinging up to five hosts, no UI
    target.rs            what to ping: the name or address typed, or the gateway from the route table
    echo.rs              one ICMP echo, timed: the ping socket on Linux and macOS, IcmpSendEcho on Windows
    pings.rs             the pings, a thread a host, --count and --wait, the totals (lost, min,
                         average, max, jitter)
  lanspeed/              the LAN throughput test, no UI
    wire.rs              what the two ends say: the hello, the server's answer, the report lines, the datagrams
    transfer.rs          sending and receiving over TCP streams and UDP sockets, counted an interval at a time
    server.rs            --serve: listen on one port, take a test at a time, receive and report or send
    runs.rs              --connect: a run (a transfer each way), the Event enum the screen hears, the medians
  dns/                   DNS lookups and whois, no UI
    servers.rs           which name servers to ask: typed, or the system's (resolv.conf, GetNetworkParams)
    lookup.rs            one question to one server over UDP or TCP, timed; the domain's own server
    whois.rs             whois over port 43: IANA, the registry, the registrar, and the fields worth showing
  wake.rs                Wake-on-LAN: the magic packet and the broadcast that sends it, no UI
  os/                    the OS outside the network
    clipboard.rs         copying through pbcopy / wl-copy / xclip / xsel / clip
  report/                the PDF reports for --report (colours from ui/theme.rs, nothing on screen)
    pdf.rs               the PDF file format: pages, shapes, text in the built-in fonts and their widths
    sheet.rs             the report's page: header card, summary, headings, tables, page breaks, footer
    sweep.rs             the sweep's report: the summary sentences and the table of devices
    wireless.rs          the survey's report: the summary, a channel chart per band, the networks
    speedtest.rs         the speed test's report: the medians, the summary, a chart a result, a row a run
    ping.rs              the ping report: the loss, the summary, a row a host, a chart a host
    dns.rs               the lookup's report: the answer, what authoritative means, the records, the whois
    lanspeed.rs          the LAN speed report: the throughput each way, a chart a direction, a row a run
  ui/                    everything on screen
    theme.rs             colour roles as ratatui styles: the terminal's own for the screen,
                         Gruvbox light for the printed report
    app.rs               the sweep's screen: state, key handling, the prompt and the table
    wireless.rs          the wireless screen: the networks, their trends, crowds and strips
    speedtest.rs         the speed test screen: a row a run, the result being measured in yellow, graphs of the runs
    ping.rs              the ping screen: each host's totals, a row a ping, a graph a host
    dns.rs               the lookup printed to the terminal: a `#` line a lookup, a row a record, the whois
    lanspeed.rs          the LAN speed screen: a row an interval, a total a direction, a graph a direction
    status_bar.rs        the bottom bar: state or notice on the left, key hints on the right
    plain.rs             the tables printed to stdout for --plain, and the words in each cell
```

One file per concern, named for that concern. When a file passes a few hundred lines, or starts
doing two jobs, split it inside its folder and keep the folder's `mod.rs` list current. Tests sit
at the bottom of the file they test.

### Rules the code can't tell you

- **The look is the cavecomputing design system's desktop section, in a terminal.** The screen is
  text laid out like a config file: `#` lines are comments and hints in `fg-subtle`, `>` in
  `accent-text` is the prompt with no box around it, the active row is a full-width `bg-inset`
  band with its text all in the plain colour, words separate with ` · `, commands are lowercase, and
  nothing on screen says what the screen is. The status bar sits on `bg-page` under a `border`
  line: a dot and the sweep's state on the left (the dot pulses while sweeping: the only looping
  motion), key hints on the right, notices in place of the state for a few seconds. No boxes, no
  borders around the table, no title.
- **The screen takes the terminal's colours.** It paints no background and sets no text colour of
  its own, so it sits in the terminal rather than as a block on it, and the terminal's theme is
  its theme (there is no theme switch). `ui/theme.rs` maps the design system's roles onto the
  terminal's 16-colour palette: `bg-surface` and `bg-page` are the terminal's background,
  `fg-subtle` and `border` are the text colour made faint, the band is bright black with its row
  bold, `accent-text` yellow, `accent-cool` blue, aqua cyan, `accent-warm` magenta. **No red.**
  **No bright black for text**: Solarized Dark (common on macOS) makes it the background, so
  anything drawn in it vanishes. Colours come from roles, never hex or a named
  colour at the call site. Every status also carries a word (`no reply`, `copy failed: ...`),
  never colour alone. The printed report keeps the Gruvbox light palette's real colours.
- **The font is the terminal's.** A desktop app of this brand bundles JetBrains Mono NL Nerd Font;
  a terminal tool can't, so the screen uses only plain Unicode it can count on: `●`, `·`, `—`,
  `─`. No Nerd Font glyphs, no emoji.
- **No branding in the UI**: no name, no logo. The name lives in the command.
- **Startup is near-instant and the screen never waits for the network.** The first frame draws
  before anything has answered; rows fill in. The maker registry is parsed inside the sweep task,
  not on the screen's thread. Dropping the sweep's receiver (Ctrl+R) ends the old sweep quietly.
- **No privileges.** `ping` uses the unprivileged ICMP datagram socket (Linux, macOS) and
  `IcmpSendEcho` (Windows), never a raw socket. Hosts are found with the system `ping` (on Windows `IcmpSendEcho`, since `ping.exe`
  prints in the system's language) plus a one-byte UDP datagram to the
  discard port (which makes the OS resolve the MAC even for hosts that ignore pings), and the MACs
  come from the OS's ARP table. Nothing opens a raw socket, so nothing needs root or setcap.
- **Names come from the host itself first.** Reverse DNS on a home or office LAN rarely knows a
  camera; the mDNS PTR query (with the unicast-response bit, so the reply comes to our socket)
  is what devices answer. Both run at once; mDNS wins.
- **macOS hides WiFi names from terminal tools** (since 14.4: `<redacted>`). Only an app the user
  has allowed Location sees them, so `wireless` on macOS runs `helper.swift` as a background
  app: the binary carries it, writes it as `<WIFI_HELPER>.app` into the data folder (ad-hoc signed,
  rewritten only when it changes), starts it with `open` so macOS treats it as its own app, and
  reads the list it writes to a temp file. A rewritten helper may need Location allowed again.
- **Shortcuts are written the way a terminal writes them** (`^r`, `esc`, `enter`), lowercase in
  the status bar. Ctrl is Ctrl on every OS inside a terminal.
- **The registry goes stale.** `assets/oui.csv` is the IEEE list on the day it was fetched;
  refresh it now and then as `assets/README.md` says. A MAC the list can't name shows `—`; a
  made-up (locally administered) one shows `randomized`, which usually means a phone.
