//! Wake-on-LAN: the magic packet that wakes a sleeping machine, broadcast over UDP. The machine's
//! network card listens for it while the rest is off, so nothing answers; sent is all we can say.

use anyhow::Context;
use std::net::{Ipv4Addr, UdpSocket};

/// The port wake packets go to unless told: the discard port, which most tools and cards use.
pub const PORT: u16 = 9;

/// The magic packet for a MAC as `normalize_mac` writes it: six 0xff bytes, then the MAC's six
/// bytes sixteen times over.
pub fn magic_packet(mac: &str) -> Vec<u8> {
    let bytes: Vec<u8> = mac
        .split(':')
        .map(|pair| u8::from_str_radix(pair, 16).expect("normalize_mac writes hex pairs"))
        .collect();
    let mut packet = vec![0xff; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&bytes);
    }
    packet
}

/// Sends the magic packet for `mac` to `address`, the broadcast address unless told otherwise.
pub fn send(mac: &str, address: Ipv4Addr, port: u16) -> anyhow::Result<()> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket
        .send_to(&magic_packet(mac), (address, port))
        .with_context(|| format!("couldn't send to {address}:{port}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_packet_is_six_ffs_then_the_mac_sixteen_times() {
        let packet = magic_packet("00:11:22:aa:bb:cc");
        assert_eq!(packet.len(), 102);
        assert_eq!(packet[..6], [0xff; 6]);
        for copy in packet[6..].chunks(6) {
            assert_eq!(copy, [0x00, 0x11, 0x22, 0xaa, 0xbb, 0xcc]);
        }
    }

    #[test]
    fn send_reaches_a_listener() {
        let listener = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a local port");
        let port = listener.local_addr().expect("its address").port();
        send("00:11:22:aa:bb:cc", Ipv4Addr::LOCALHOST, port).expect("sent");
        let mut buffer = [0; 200];
        let length = listener.recv(&mut buffer).expect("received");
        assert_eq!(buffer[..length], magic_packet("00:11:22:aa:bb:cc"));
    }
}
