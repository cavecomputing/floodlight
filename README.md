# Floodlight

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
floodlight                      # the subnet this machine is on
floodlight 192.168.1.0/24       # a subnet, or an address for its /24
floodlight --plain              # print the table and exit
floodlight --find 3d:57         # print only the matching rows
```

On screen: type to filter (an address fragment, a MAC with or without separators, a maker, a
name), ↑↓ to move, **enter** copies the address, **tab** copies the MAC, **^r** sweeps again,
**^t** switches between the dark and light theme, **esc** clears, **^c** quits.

Needs no root: hosts are found with the system `ping` and the OS's ARP table, names with mDNS and
reverse DNS, makers from the IEEE registry built into the binary. Linux, macOS and Windows.

## Build

```bash
cargo install --path .
```

Rust (rustup) and a C compiler are all it takes to build. On Linux, copying needs `wl-copy`
(wl-clipboard), `xclip` or `xsel`.

## Licence

MIT. The maker registry in `assets/oui.csv` is the IEEE's public MA-L list.
