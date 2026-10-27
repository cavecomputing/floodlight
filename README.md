# Floodlight

Looks at the network around this machine. Each feature has its flag; `floodlight` alone lists
them.

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
```

On screen: type to filter (an address fragment, a MAC with or without separators, a maker, a
name), ↑↓ to move, **enter** copies the address, **tab** copies the MAC, **^r** sweeps again,
**^t** switches between the dark and light theme, **esc** clears, **^c** quits.

Needs no root: hosts are found with the system `ping` and the OS's ARP table, names with mDNS and
reverse DNS, makers from the IEEE registry built into the binary. Linux, macOS and Windows.

## --wireless

Lists every WiFi network this machine can hear, with its channel, width and signal, how many
neighbours share its channel (they take turns) or overlap it from a nearby one (they talk over
each other, which is worse), and a strip of its band so the rows stacked together draw the
overlap. The top line names the quietest channel in each band.

```
  # 8 networks · quietest 2.4 GHz 11 · 5 GHz 44

  # ssid          ch   band     width    signal   neighbours          air
    Home · joined 6    2.4 GHz  —        -59 dBm  1 same · 1 overlap  ···──●──·····
    Neighbour     6    2.4 GHz  —        -80 dBm  1 same · 1 overlap  ···──●──·····
    Cafe Guest    8    2.4 GHz  —        -73 dBm  3 overlap           ·····──●──···
```

```bash
floodlight --wireless           # the screen; ^r scans again
floodlight --wireless --plain   # print the table and exit
```

Read from the OS's own tool, no root: `nmcli` (NetworkManager) or `iw` on Linux (with iwd, as on
Omarchy, install `iw`; iwd is asked for a fresh scan first), `system_profiler` on macOS, `netsh`
on Windows. A width the OS doesn't report (nmcli, netsh) shows `—` and counts as 20 MHz. macOS
and recent Windows may hide network names until the terminal has location access.

## Build

```bash
cargo install --path .
```

Rust (rustup) and a C compiler are all it takes to build. On Linux, copying needs `wl-copy`
(wl-clipboard), `xclip` or `xsel`.

## Licence

MIT. The maker registry in `assets/oui.csv` is the IEEE's public MA-L list.
