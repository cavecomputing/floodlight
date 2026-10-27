//! One ICMP echo, without root or Administrator: the OS's own ping socket on Linux and macOS,
//! Windows' own ping call (iphlpapi) on Windows. Each round trip is timed here rather than taken
//! from the OS, because Windows only counts whole milliseconds.

#[cfg(unix)]
pub use unix::Pinger;
#[cfg(windows)]
pub use windows::Pinger;

/// What each echo carries: 32 bytes, as Windows' own ping sends.
const PAYLOAD: &[u8; 32] = b"abcdefghijklmnopqrstuvwabcdefghi";

/// An echo request: type 8, code 0, the checksum, our identifier and the sequence number, then the
/// payload. Windows builds its own, so this goes unused there.
#[cfg_attr(windows, allow(dead_code))]
fn request(id: u16, seq: u16) -> Vec<u8> {
    let mut packet = vec![8, 0, 0, 0];
    packet.extend(id.to_be_bytes());
    packet.extend(seq.to_be_bytes());
    packet.extend(PAYLOAD);
    let sum = checksum(&packet);
    packet[2..4].copy_from_slice(&sum.to_be_bytes());
    packet
}

/// The internet checksum: the bytes summed as 16-bit words with the carries folded back in, then
/// flipped. A packet with its checksum in place sums to zero.
#[cfg_attr(windows, allow(dead_code))]
fn checksum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = bytes
        .chunks(2)
        .map(|pair| u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]) as u32)
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Whether a packet that came in is the reply to echo `seq`. macOS hands it over with the IP
/// header in front; Linux doesn't, and puts its own identifier on the echo (it only hands this
/// socket its own replies, so there is nothing to check it against).
#[cfg_attr(windows, allow(dead_code))]
fn is_reply(packet: &[u8], id: u16, seq: u16) -> bool {
    let icmp = match packet.first() {
        Some(byte) if byte >> 4 == 4 => &packet[(usize::from(byte & 0x0f) * 4).min(packet.len())..],
        _ => packet,
    };
    icmp.len() >= 8
        && icmp[0] == 0
        && (cfg!(target_os = "linux") || icmp[4..6] == id.to_be_bytes())
        && icmp[6..8] == seq.to_be_bytes()
}

#[cfg(unix)]
mod unix {
    use super::{is_reply, request};
    use socket2::{Domain, Protocol, Socket, Type};
    use std::io::ErrorKind;
    use std::net::{Ipv4Addr, UdpSocket};
    use std::time::{Duration, Instant};

    pub struct Pinger {
        /// An ICMP datagram socket, which std's UDP socket can send and receive on as it is.
        socket: UdpSocket,
        id: u16,
    }

    impl Pinger {
        pub fn new() -> Result<Pinger, String> {
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(
                |why| match cfg!(target_os = "linux") {
                    true => format!(
                        "this system doesn't let programs ping without root ({why}); allow it \
                         with: sudo sysctl -w net.ipv4.ping_group_range=\"0 2147483647\""
                    ),
                    false => format!("couldn't open a ping socket: {why}"),
                },
            )?;
            Ok(Pinger {
                socket: socket.into(),
                id: std::process::id() as u16,
            })
        }

        /// Sends echo `seq` and waits up to `timeout` for its reply. Replies to earlier echoes
        /// that come in late are passed over.
        pub fn echo(&self, ip: Ipv4Addr, seq: u16, timeout: Duration) -> Result<Duration, String> {
            let sent = Instant::now();
            self.socket
                .send_to(&request(self.id, seq), (ip, 0))
                .map_err(failed)?;
            let mut packet = [0; 1500];
            loop {
                let left = timeout.saturating_sub(sent.elapsed());
                if left.is_zero() {
                    return Err("timed out".to_string());
                }
                self.socket.set_read_timeout(Some(left)).map_err(failed)?;
                let (size, from) = match self.socket.recv_from(&mut packet) {
                    Ok(received) => received,
                    Err(why)
                        if matches!(why.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        return Err("timed out".to_string());
                    }
                    Err(why) => return Err(failed(why)),
                };
                let took = sent.elapsed();
                if from.ip() == ip && is_reply(&packet[..size], self.id, seq) {
                    return Ok(took);
                }
            }
        }
    }

    /// Why an echo failed, in the words Windows' side uses for the common ones.
    fn failed(why: std::io::Error) -> String {
        match why.kind() {
            ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable => "unreachable".to_string(),
            _ => why.to_string(),
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::PAYLOAD;
    use std::net::Ipv4Addr;
    use std::ptr::null;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        ICMP_ECHO_REPLY, IP_DEST_HOST_UNREACHABLE, IP_DEST_NET_UNREACHABLE, IP_REQ_TIMED_OUT,
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
    };

    pub struct Pinger {
        handle: HANDLE,
    }

    // SAFETY: an ICMP handle is a plain handle Windows lets any thread use; only the ping thread
    // uses this one once it is made.
    unsafe impl Send for Pinger {}

    /// The reply with room after it for the payload coming back, as IcmpSendEcho asks.
    #[repr(C)]
    struct Answer {
        reply: ICMP_ECHO_REPLY,
        room: [u8; 64],
    }

    impl Pinger {
        pub fn new() -> Result<Pinger, String> {
            // SAFETY: takes nothing; the handle is checked before use and closed on drop.
            let handle = unsafe { IcmpCreateFile() };
            if handle == INVALID_HANDLE_VALUE {
                return Err("windows wouldn't open its ping service (iphlpapi)".to_string());
            }
            Ok(Pinger { handle })
        }

        /// Sends one echo and waits up to `timeout` for the reply. Windows matches replies to
        /// echoes itself, so `seq` is only for the other OSes.
        pub fn echo(&self, ip: Ipv4Addr, _seq: u16, timeout: Duration) -> Result<Duration, String> {
            // SAFETY: a zeroed reply is a valid one: numbers and a null pointer.
            let mut answer: Answer = unsafe { std::mem::zeroed() };
            let sent = Instant::now();
            // SAFETY: the payload and the answer are live locals, and their sizes are the ones given.
            let replies = unsafe {
                IcmpSendEcho(
                    self.handle,
                    u32::from_ne_bytes(ip.octets()),
                    PAYLOAD.as_ptr().cast(),
                    PAYLOAD.len() as u16,
                    null(),
                    (&mut answer as *mut Answer).cast(),
                    size_of::<Answer>() as u32,
                    timeout.as_millis() as u32,
                )
            };
            let took = sent.elapsed();
            let status = match replies {
                // SAFETY: reads the error IcmpSendEcho just left on this thread.
                0 => unsafe { GetLastError() },
                _ => answer.reply.Status,
            };
            match status {
                0 => Ok(took),
                IP_REQ_TIMED_OUT => Err("timed out".to_string()),
                IP_DEST_HOST_UNREACHABLE | IP_DEST_NET_UNREACHABLE => {
                    Err("unreachable".to_string())
                }
                code => Err(format!("windows error {code}")),
            }
        }
    }

    impl Drop for Pinger {
        fn drop(&mut self) {
            // SAFETY: the handle came from IcmpCreateFile and is closed only here.
            unsafe { IcmpCloseHandle(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_echo_request_sums_to_zero() {
        let packet = request(0x1234, 7);
        assert_eq!(&packet[..2], &[8, 0]);
        assert_eq!(&packet[4..8], &[0x12, 0x34, 0, 7]);
        assert_eq!(packet.len(), 8 + PAYLOAD.len());
        assert_eq!(checksum(&packet), 0);
    }

    #[test]
    fn knows_its_reply_with_or_without_the_ip_header() {
        let mut reply = request(0x1234, 7);
        reply[0] = 0;
        // Linux: the ICMP message alone.
        assert!(is_reply(&reply, 0x1234, 7));
        assert!(!is_reply(&reply, 0x1234, 8));
        // macOS: a 20-byte IPv4 header in front.
        let mut with_header = vec![0x45; 1];
        with_header.extend([0; 19]);
        with_header.extend(&reply);
        assert!(is_reply(&with_header, 0x1234, 7));
        // An echo request (ours, looped back) isn't a reply.
        assert!(!is_reply(&request(0x1234, 7), 0x1234, 7));
        assert!(!is_reply(&[0, 0, 0], 0x1234, 7));
    }
}
