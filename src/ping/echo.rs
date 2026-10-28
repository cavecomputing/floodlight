//! One ICMP echo, without root or Administrator: the OS's own ping socket on Linux and macOS,
//! Windows' own ping call (iphlpapi) on Windows. Each round trip is timed here rather than taken
//! from the OS, because Windows only counts whole milliseconds. With a TTL set, the router where
//! it runs out answers instead of the host, which is how `trace` finds each hop.

use std::net::Ipv4Addr;
use std::time::Duration;

#[cfg(unix)]
pub use unix::Pinger;
#[cfg(windows)]
pub use windows::Pinger;

/// Who answered an echo and how long it took: the host pinged, or the router where the echo's
/// TTL ran out on the way there.
pub struct Answer {
    pub from: Ipv4Addr,
    pub took: Duration,
}

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

/// ICMP's types for an echo reply, a destination unreachable and a time (to live) exceeded.
const ECHO_REPLY: u8 = 0;
const UNREACHABLE: u8 = 3;
const TIME_EXCEEDED: u8 = 11;

/// The ICMP type of a packet that came in, when it answers echo `seq`: the reply itself, or a
/// router's word that it was unreachable or its TTL ran out, which quotes the echo after the
/// echo's own IP header. macOS hands every packet over with the IP header in front; Linux doesn't,
/// hands this socket only its own replies, and puts its own identifier on the echo (so there is
/// nothing to check it against).
#[cfg_attr(windows, allow(dead_code))]
fn answer_to(packet: &[u8], id: u16, seq: u16) -> Option<u8> {
    let icmp = after_ip_header(packet);
    let kind = *icmp.first()?;
    let echo = match kind {
        ECHO_REPLY => icmp,
        UNREACHABLE | TIME_EXCEEDED => after_ip_header(icmp.get(8..)?),
        _ => return None,
    };
    let ours = echo.len() >= 8
        && echo[0] == if kind == ECHO_REPLY { ECHO_REPLY } else { 8 }
        && (cfg!(target_os = "linux") || echo[4..6] == id.to_be_bytes())
        && echo[6..8] == seq.to_be_bytes();
    ours.then_some(kind)
}

/// A packet past its IPv4 header, if it starts with one.
#[cfg_attr(windows, allow(dead_code))]
fn after_ip_header(packet: &[u8]) -> &[u8] {
    match packet.first() {
        Some(byte) if byte >> 4 == 4 => &packet[(usize::from(byte & 0x0f) * 4).min(packet.len())..],
        _ => packet,
    }
}

#[cfg(unix)]
mod unix {
    use super::{Answer, ECHO_REPLY, TIME_EXCEEDED, UNREACHABLE, answer_to, request};
    use socket2::{Domain, Protocol, Socket, Type};
    use std::io::ErrorKind;
    use std::net::{Ipv4Addr, UdpSocket};
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::{Duration, Instant};

    /// Pingers made so far, to set each one's identifier apart.
    static NEXT: AtomicU16 = AtomicU16::new(0);

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
                // macOS hands every ping socket every reply, so each pinger of this process
                // (one a host) gets an identifier of its own.
                id: (std::process::id() as u16).wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed)),
            })
        }

        /// Sends every echo with this TTL, so the router where it runs out answers instead of
        /// the host when the host is further away.
        pub fn set_ttl(&mut self, ttl: u8) -> Result<(), String> {
            self.socket.set_ttl(ttl.into()).map_err(failed)?;
            // Linux tells a ping socket about a router's answer only when asked to, on the
            // socket's error queue. macOS hands it over as one more packet.
            #[cfg(target_os = "linux")]
            linux::ask_for_errors(&self.socket).map_err(failed)?;
            Ok(())
        }

        /// Sends echo `seq` and waits up to `timeout` for its reply, or a router's word that its
        /// TTL ran out. Answers to earlier echoes that come in late are passed over.
        pub fn echo(&self, ip: Ipv4Addr, seq: u16, timeout: Duration) -> Result<Answer, String> {
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
                    Err(why) => {
                        // A router's answer on Linux's error queue; an earlier echo's is passed
                        // over. Nothing there (no TTL set) is the error itself.
                        #[cfg(target_os = "linux")]
                        if let Some(heard) = linux::queued(&self.socket, seq) {
                            match heard {
                                Some((TIME_EXCEEDED, from)) => {
                                    let took = sent.elapsed();
                                    return Ok(Answer { from, took });
                                }
                                Some(_) => return Err("unreachable".to_string()),
                                None => continue,
                            }
                        }
                        return Err(failed(why));
                    }
                };
                let took = sent.elapsed();
                let std::net::IpAddr::V4(from) = from.ip() else {
                    continue;
                };
                match answer_to(&packet[..size], self.id, seq) {
                    Some(ECHO_REPLY) if from == ip => return Ok(Answer { from, took }),
                    Some(TIME_EXCEEDED) => return Ok(Answer { from, took }),
                    Some(UNREACHABLE) => return Err("unreachable".to_string()),
                    _ => {}
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

    /// Linux's error queue, where a ping socket that asked (IP_RECVERR) hears from a router.
    #[cfg(target_os = "linux")]
    mod linux {
        use std::net::{Ipv4Addr, UdpSocket};
        use std::os::fd::AsRawFd;

        pub fn ask_for_errors(socket: &UdpSocket) -> std::io::Result<()> {
            let on: libc::c_int = 1;
            // SAFETY: the socket is open, and the option's value is a live int of the size given.
            let set = unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_IP,
                    libc::IP_RECVERR,
                    (&on as *const libc::c_int).cast(),
                    size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
            match set {
                0 => Ok(()),
                _ => Err(std::io::Error::last_os_error()),
            }
        }

        /// Takes everything off the error queue: `None` when it was empty, else the ICMP type and
        /// the router of the entry for echo `seq`, if one was there.
        pub fn queued(socket: &UdpSocket, seq: u16) -> Option<Option<(u8, Ipv4Addr)>> {
            let mut found = None;
            let mut any = false;
            loop {
                // The echo that failed (its ICMP header is enough), and the note about why.
                let mut echo = [0u8; 64];
                let mut note = [0u8; 256];
                let mut part = libc::iovec {
                    iov_base: echo.as_mut_ptr().cast(),
                    iov_len: echo.len(),
                };
                // SAFETY: a zeroed message header is a valid empty one; its buffers are set next.
                let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
                message.msg_iov = &mut part;
                message.msg_iovlen = 1;
                message.msg_control = note.as_mut_ptr().cast();
                message.msg_controllen = note.len() as _;
                // SAFETY: every buffer the header points at is a live local of the size it says.
                let size = unsafe {
                    libc::recvmsg(
                        socket.as_raw_fd(),
                        &mut message,
                        libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT,
                    )
                };
                if size < 0 {
                    return any.then_some(found);
                }
                any = true;
                let ours = size >= 8 && echo[6..8] == seq.to_be_bytes();
                // SAFETY: the kernel filled `note` with whole control messages, which these
                // macros walk; the error and the router's address after it are read unaligned.
                unsafe {
                    let mut header = libc::CMSG_FIRSTHDR(&message);
                    while ours && !header.is_null() {
                        if (*header).cmsg_level == libc::SOL_IP
                            && (*header).cmsg_type == libc::IP_RECVERR
                        {
                            let error = libc::CMSG_DATA(header).cast::<libc::sock_extended_err>();
                            let error_read = error.read_unaligned();
                            if error_read.ee_origin == libc::SO_EE_ORIGIN_ICMP {
                                let router =
                                    error.add(1).cast::<libc::sockaddr_in>().read_unaligned();
                                let from = Ipv4Addr::from(router.sin_addr.s_addr.to_ne_bytes());
                                found = Some((error_read.ee_type, from));
                            }
                        }
                        header = libc::CMSG_NXTHDR(&message, header);
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::{Answer, PAYLOAD};
    use std::net::Ipv4Addr;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        ICMP_ECHO_REPLY, IP_DEST_HOST_UNREACHABLE, IP_DEST_NET_UNREACHABLE, IP_OPTION_INFORMATION,
        IP_REQ_TIMED_OUT, IP_TTL_EXPIRED_TRANSIT, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
    };

    pub struct Pinger {
        handle: HANDLE,
        /// Windows' own default until `set_ttl`.
        ttl: u8,
    }

    // SAFETY: an ICMP handle is a plain handle Windows lets any thread use; only the ping thread
    // uses this one once it is made.
    unsafe impl Send for Pinger {}

    /// The reply with room after it for the payload coming back, as IcmpSendEcho asks.
    #[repr(C)]
    struct Reply {
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
            Ok(Pinger { handle, ttl: 128 })
        }

        /// Sends every echo with this TTL, so the router where it runs out answers instead of
        /// the host when the host is further away.
        pub fn set_ttl(&mut self, ttl: u8) -> Result<(), String> {
            self.ttl = ttl;
            Ok(())
        }

        /// Sends one echo and waits up to `timeout` for the reply, or a router's word that its TTL
        /// ran out. Windows matches replies to echoes itself, so `seq` is only for the other OSes.
        pub fn echo(&self, ip: Ipv4Addr, _seq: u16, timeout: Duration) -> Result<Answer, String> {
            // SAFETY: a zeroed reply is a valid one: numbers and a null pointer.
            let mut answer: Reply = unsafe { std::mem::zeroed() };
            // SAFETY: as above.
            let mut options: IP_OPTION_INFORMATION = unsafe { std::mem::zeroed() };
            options.Ttl = self.ttl;
            let sent = Instant::now();
            // SAFETY: the payload and the answer are live locals, and their sizes are the ones given.
            let replies = unsafe {
                IcmpSendEcho(
                    self.handle,
                    u32::from_ne_bytes(ip.octets()),
                    PAYLOAD.as_ptr().cast(),
                    PAYLOAD.len() as u16,
                    &options,
                    (&mut answer as *mut Reply).cast(),
                    size_of::<Reply>() as u32,
                    timeout.as_millis() as u32,
                )
            };
            let took = sent.elapsed();
            let status = match replies {
                // SAFETY: reads the error IcmpSendEcho just left on this thread.
                0 => unsafe { GetLastError() },
                _ => answer.reply.Status,
            };
            let from = Ipv4Addr::from(answer.reply.Address.to_ne_bytes());
            match status {
                0 | IP_TTL_EXPIRED_TRANSIT => Ok(Answer { from, took }),
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
        assert_eq!(answer_to(&reply, 0x1234, 7), Some(ECHO_REPLY));
        assert_eq!(answer_to(&reply, 0x1234, 8), None);
        // macOS: a 20-byte IPv4 header in front.
        let header = [0x45].into_iter().chain([0; 19]);
        let with_header: Vec<u8> = header.clone().chain(reply).collect();
        assert_eq!(answer_to(&with_header, 0x1234, 7), Some(ECHO_REPLY));
        // An echo request (ours, looped back) isn't a reply.
        assert_eq!(answer_to(&request(0x1234, 7), 0x1234, 7), None);
        assert_eq!(answer_to(&[0, 0, 0], 0x1234, 7), None);
    }

    #[test]
    fn knows_a_router_answering_for_its_echo() {
        // macOS: the router's IP header, its time exceeded, then our echo's IP header and echo.
        let header = || [0x45].into_iter().chain([0; 19]);
        let exceeded: Vec<u8> = header()
            .chain([TIME_EXCEEDED, 0, 0, 0, 0, 0, 0, 0])
            .chain(header())
            .chain(request(0x1234, 7))
            .collect();
        assert_eq!(answer_to(&exceeded, 0x1234, 7), Some(TIME_EXCEEDED));
        assert_eq!(answer_to(&exceeded, 0x1234, 6), None);
        let mut unreachable = exceeded.clone();
        unreachable[20] = UNREACHABLE;
        assert_eq!(answer_to(&unreachable, 0x1234, 7), Some(UNREACHABLE));
        // Cut short before the echo it quotes: not ours to claim.
        assert_eq!(answer_to(&exceeded[..40], 0x1234, 7), None);
    }
}
