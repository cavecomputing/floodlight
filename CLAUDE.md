# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

The app is **Floodlight**, a terminal tool written in Rust. Each feature is a flag. `--sweep`
sweeps a subnet and shows every address that answered with its MAC, maker and name, under a prompt
that filters the rows as you type, so a camera or switch you just plugged in is found by the MAC
on its label (or the other way round). `--wireless` lists the WiFi networks in earshot and how
their channels overlap. Linux (the owner's Omarchy/Hyprland machines) comes first, and it also builds and runs on
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
  `Cargo.toml` (every dependency there has one). Prefer the OS's own tool over a crate that links
  against a system library: the sweep shells out to `ping`, `arp`/`ip` and the clipboard tools
  for exactly that reason, and needs no root.
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
cargo run                              # build (debug) and print the help: every feature is a flag
cargo run -- --sweep                   # sweep the subnet this machine is on
cargo run -- --sweep 192.168.1.0/24    # sweep that subnet (everything after -- goes to the app)
cargo run -- --sweep --plain           # print the table and exit (for pipes, scripts, a quick look)
cargo run -- --sweep --find 3d:57      # print only the rows matching an ip, mac, maker or name fragment
cargo run -- --wireless                # the WiFi networks heard here and how their channels overlap
cargo run -- --wireless --plain        # the same as a table on stdout
cargo run -- --sweep --report out.pdf  # one sweep written as a PDF report, then exit (--wireless too)
cargo run -- --wireless --report out.pdf --scan-for 20  # scan 20 s before writing (default 10)
cargo run -- --wireless --live         # keep scanning, with each access point's signal trend

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
For `--wireless`: `nmcli` or `iw` on Linux, `netsh` on Windows; on macOS the build compiles a
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
- Logic (matching, MAC normalising, the ARP and ping parsers, the mDNS packet, settings, the WiFi
  parsers and channel maths): unit tests next to the code, in a `#[cfg(test)] mod tests` block at
  the bottom of the file. Keep that logic out of `ui/` so it can be tested without a terminal.
- Anything visual (layout, colours, the status bar): run the app in a terminal and look, in both
  themes (Ctrl+T). `--plain` checks the sweep itself without a screen. Never claim a change was
  checked on screen when it wasn't.
- The sweep touches a real network. Run it on the LAN you are on; don't put its output (real
  addresses and MACs) in the repo.

## Architecture

Each feature is a flag, and the command alone prints the help listing them.

`--sweep`: `floodlight --sweep [subnet]` opens the screen, starts the sweep in the
background, and fills the table as answers come in. Ctrl+R sweeps again.

`--report FILE`: with either feature, runs it once, writes `FILE` as a PDF and exits. The PDF is
written by hand in `report/` (no crate): the built-in Helvetica and Courier, which every reader
has, so nothing is embedded. It follows the design system's print documents (the light theme on a
cream Letter page, yellow edges, small-capital headings, rounded tables) with **no branding**: no
name or logo anywhere on it.

`--wireless`: `floodlight --wireless` opens a screen of every network heard, asks the OS's own
tool for the list once in the background, and fills in when it answers; Ctrl+R asks again.
With `--live` it asks again a second after each answer, keeping the last rows up meanwhile, and
`wifi/trend.rs` tracks each access point's signal across scans.

There is no daemon, no
state between runs beyond `settings.toml`.

The screen is [ratatui](https://ratatui.rs): every tick (50ms) it drains the sweep's channel into
`Hosts`, redraws, and handles one key press. The sweep runs on a tokio runtime on other threads
and only ever talks to the screen through `Event`s.

### Folder layout

The code is split by *what it knows about*. The rule that keeps it organised: **`scan/`, `wifi/`
and `os/` never import ratatui**, so all the logic runs in tests, and `ui/` holds only drawing and
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
    ping.rs              one address: the UDP poke and the system ping, and reading its round trip
    arp.rs               the OS's ARP table in its three formats (arp -an, ip neigh, Windows arp -a)
    mdns.rs              a reverse lookup over mDNS: the PTR question packet and reading the reply
    vendor.rs            the maker from a MAC's first three bytes; randomized addresses; tidied names
    hosts.rs             a Host row, the Hosts table built from Events, MAC normalising, and how a
                         Query matches a row (address, MAC with or without separators, maker, name)
  wifi/                  the WiFi survey, no UI
    survey.rs            the networks heard, from nmcli / iw / netsh / the macOS helper, a parser each
    helper.swift         the macOS helper app: names need Location, which only an app can be given
    channels.rs          channels as frequencies: bands, the slice a network fills, who shares or
                         overlaps it, the quietest channel, the strip that draws a band
    trend.rs             each access point's signal across scans: change and peak, for --live
  os/                    the OS outside the network
    settings.rs          settings.toml: load, defaults, atomic save
    clipboard.rs         copying through pbcopy / wl-copy / xclip / xsel / clip
  report/                the PDF reports for --report (colours from ui/theme.rs, nothing on screen)
    pdf.rs               the PDF file format: pages, shapes, text in the built-in fonts and their widths
    sheet.rs             the report's page: header card, summary, headings, tables, page breaks, footer
    sweep.rs             the sweep's report: the summary sentences and the table of devices
    wireless.rs          the survey's report: the summary, a channel chart per band, the networks
  ui/                    everything on screen
    theme.rs             Gruvbox palette roles as ratatui styles
    app.rs               the sweep's screen: state, key handling, the prompt and the table
    wireless.rs          the wireless screen: the networks, their crowds and strips, live or once
    status_bar.rs        the bottom bar: state or notice on the left, key hints on the right
    plain.rs             the tables printed to stdout for --plain and --find, and the words in each cell
```

One file per concern, named for that concern. When a file passes a few hundred lines, or starts
doing two jobs, split it inside its folder and keep the folder's `mod.rs` list current. Tests sit
at the bottom of the file they test.

### Rules the code can't tell you

- **The look is the cavecomputing design system's desktop section, in a terminal.** The screen is
  text laid out like a config file: `#` lines are comments and hints in `fg-subtle`, `>` in
  `accent-text` is the prompt with no box around it, the active row is a full-width `bg-inset`
  band with its address in `accent-text`, words separate with ` · `, commands are lowercase, and
  nothing on screen says what the screen is. The status bar sits on `bg-page` under a `border`
  line: a dot and the sweep's state on the left (the dot pulses while sweeping: the only looping
  motion), key hints on the right, notices in place of the state for a few seconds. No boxes, no
  borders around the table, no title.
- **Colours come from roles, not hex.** `ui/theme.rs` maps the design system's Gruvbox tokens to
  roles: `bg-surface` the screen, `bg-page` the status bar, `accent-cool` addresses,
  `accent-text` the prompt, the active row and a match, aqua a host that answered or a copy that
  worked, `accent-warm` anything that went wrong. **No red.** Every status also carries a word
  (`no reply`, `copy failed: ...`), never colour alone. Both themes are full themes; dark is the
  default, Ctrl+T switches and saves.
- **The font is the terminal's.** A desktop app of this brand bundles JetBrains Mono NL Nerd Font;
  a terminal tool can't, so the screen uses only plain Unicode it can count on: `●`, `·`, `—`,
  `─`. No Nerd Font glyphs, no emoji.
- **No branding in the UI**: no name, no logo. The name lives in the command.
- **Startup is near-instant and the screen never waits for the network.** The first frame draws
  before anything has answered; rows fill in. The maker registry is parsed inside the sweep task,
  not on the screen's thread. Dropping the sweep's receiver (Ctrl+R) ends the old sweep quietly.
- **No privileges.** Hosts are found with the system `ping` plus a one-byte UDP datagram to the
  discard port (which makes the OS resolve the MAC even for hosts that ignore pings), and the MACs
  come from the OS's ARP table. Nothing opens a raw socket, so nothing needs root or setcap.
- **Names come from the host itself first.** Reverse DNS on a home or office LAN rarely knows a
  camera; the mDNS PTR query (with the unicast-response bit, so the reply comes to our socket)
  is what devices answer. Both run at once; mDNS wins.
- **macOS hides WiFi names from terminal tools** (since 14.4: `<redacted>`). Only an app the user
  has allowed Location sees them, so `--wireless` on macOS runs `helper.swift` as a background
  app: the binary carries it, writes it as `<WIFI_HELPER>.app` into the data folder (ad-hoc signed,
  rewritten only when it changes), starts it with `open` so macOS treats it as its own app, and
  reads the list it writes to a temp file. A rewritten helper may need Location allowed again.
- **Settings** live in `settings.toml` (hand-editable) in the OS's config folder:
  `~/.config/<config dir>/` on Linux, `~/Library/Application Support/<config dir>/` on macOS,
  `%APPDATA%\<config dir>\config\` on Windows. Unknown or missing keys fall back to defaults; a
  save is a temp file renamed over the target.
- **Shortcuts are written the way a terminal writes them** (`^r`, `esc`, `enter`), lowercase in
  the status bar. Ctrl is Ctrl on every OS inside a terminal.
- **The registry goes stale.** `assets/oui.csv` is the IEEE list on the day it was fetched;
  refresh it now and then as `assets/README.md` says. A MAC the list can't name shows `—`; a
  made-up (locally administered) one shows `randomized`, which usually means a phone.
