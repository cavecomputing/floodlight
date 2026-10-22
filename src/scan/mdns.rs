//! A reverse lookup over mDNS: asks the whole subnet who has the address, which is what cameras,
//! printers, speakers and laptops answer on a LAN where no DNS server knows them.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout_at};

const GROUP: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(224, 0, 0, 251), 5353);
const PTR: u16 = 12;

/// The name the host gives for itself, or None when nothing answers in time.
pub async fn name(ip: Ipv4Addr, wait: Duration) -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    socket.send_to(&question(ip), GROUP).await.ok()?;
    let deadline = Instant::now() + wait;
    let mut packet = [0u8; 1500];
    loop {
        let (len, _) = timeout_at(deadline, socket.recv_from(&mut packet))
            .await
            .ok()?
            .ok()?;
        if let Some(found) = answer(&packet[..len]) {
            return Some(found);
        }
    }
}

/// A PTR question for `d.c.b.a.in-addr.arpa`. The top bit of the class asks for the reply to
/// come back to this socket alone instead of to the whole group.
pub fn question(ip: Ipv4Addr) -> Vec<u8> {
    let mut packet = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    let [a, b, c, d] = ip.octets();
    let labels = [d.to_string(), c.to_string(), b.to_string(), a.to_string()];
    for label in labels.iter().map(String::as_str).chain(["in-addr", "arpa"]) {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&[0, PTR as u8, 0x80, 1]);
    packet
}

/// The name in the first PTR answer of a reply.
pub fn answer(packet: &[u8]) -> Option<String> {
    let questions = u16::from_be_bytes([*packet.get(4)?, *packet.get(5)?]) as usize;
    let answers = u16::from_be_bytes([*packet.get(6)?, *packet.get(7)?]) as usize;
    let mut at = 12;
    for _ in 0..questions {
        at = past_name(packet, at)? + 4;
    }
    for _ in 0..answers {
        at = past_name(packet, at)?;
        let kind = u16::from_be_bytes([*packet.get(at)?, *packet.get(at + 1)?]);
        let length = u16::from_be_bytes([*packet.get(at + 8)?, *packet.get(at + 9)?]) as usize;
        let data = at + 10;
        if kind == PTR {
            return read_name(packet, data).filter(|name| !name.is_empty());
        }
        at = data + length;
    }
    None
}

/// Where the name that starts at `at` ends: after its labels, or after a two-byte pointer.
fn past_name(packet: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let len = *packet.get(at)? as usize;
        if len == 0 {
            return Some(at + 1);
        }
        if len & 0xc0 == 0xc0 {
            return Some(at + 2);
        }
        at += 1 + len;
    }
}

/// The dotted name at `at`, following pointers to names earlier in the packet (a few at most,
/// so a packet that points in a circle gives up).
fn read_name(packet: &[u8], mut at: usize) -> Option<String> {
    let mut labels = Vec::new();
    let mut jumps = 0;
    loop {
        let len = *packet.get(at)? as usize;
        if len == 0 {
            break;
        }
        if len & 0xc0 == 0xc0 {
            jumps += 1;
            if jumps > 8 {
                return None;
            }
            at = ((len & 0x3f) << 8) | *packet.get(at + 1)? as usize;
            continue;
        }
        let label = packet.get(at + 1..at + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        at += 1 + len;
    }
    Some(labels.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_the_reversed_address() {
        let packet = question(Ipv4Addr::new(192, 168, 1, 108));
        let mut expected = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in ["108", "1", "168", "192", "in-addr", "arpa"] {
            expected.push(label.len() as u8);
            expected.extend_from_slice(label.as_bytes());
        }
        expected.extend_from_slice(&[0, 0, 12, 0x80, 1]);
        assert_eq!(packet, expected);
    }

    #[test]
    fn reads_a_pointer_compressed_reply() {
        // A reply that repeats the question, then answers with `doorcam.local` where `local`
        // is a pointer back to the same word written earlier.
        let mut packet = vec![0, 0, 0x84, 0, 0, 1, 0, 1, 0, 0, 0, 0];
        let name_at = packet.len();
        for label in ["108", "1", "168", "192", "in-addr", "arpa"] {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.extend_from_slice(&[0, 0, 12, 0, 1]);
        let pointer = 0xc000 | name_at as u16;
        packet.extend_from_slice(&pointer.to_be_bytes());
        packet.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0, 120]);
        let local_at = packet.len() + 2 + 1 + "doorcam".len();
        let rdata_len = 1 + "doorcam".len() + 1 + "local".len() + 1;
        packet.extend_from_slice(&(rdata_len as u16).to_be_bytes());
        packet.push(7);
        packet.extend_from_slice(b"doorcam");
        packet.push(5);
        packet.extend_from_slice(b"local");
        packet.push(0);
        assert_eq!(answer(&packet).as_deref(), Some("doorcam.local"));

        // The same answer with `local` as a pointer to a copy of the label two bytes on.
        let mut pointed = packet[..local_at].to_vec();
        let pointer = 0xc000 | (local_at + 2) as u16;
        pointed.extend_from_slice(&pointer.to_be_bytes());
        pointed.push(5);
        pointed.extend_from_slice(b"local");
        pointed.push(0);
        assert_eq!(answer(&pointed).as_deref(), Some("doorcam.local"));

        // A pointer at itself goes round in circles and gives up.
        let mut circular = packet[..local_at].to_vec();
        let pointer = 0xc000 | local_at as u16;
        circular.extend_from_slice(&pointer.to_be_bytes());
        assert_eq!(answer(&circular), None);
    }

    #[test]
    fn a_short_or_empty_packet_is_no_answer() {
        assert_eq!(answer(&[]), None);
        assert_eq!(answer(&[0, 0, 0x84, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    }
}
