//! What the two ends say to each other: a hello line asking for a test, the server's answer,
//! the lines a receiver reports each interval, and the UDP datagrams. Lines are `word key=value
//! ...`, so either end can be read by eye with a packet capture.

use std::net::SocketAddr;
use std::time::Duration;

/// Bumped when a line or datagram changes shape, so an older copy is refused rather than misread.
pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        }
    }
}

/// Which way the data goes, from the client's point of view: upload sends to the server.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    Upload,
    Download,
}

impl Direction {
    pub fn name(self) -> &'static str {
        match self {
            Direction::Upload => "upload",
            Direction::Download => "download",
        }
    }
}

/// One transfer, one way, as the client asks for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Test {
    pub protocol: Protocol,
    pub direction: Direction,
    /// Connections (or UDP sockets) sending at once.
    pub streams: usize,
    pub seconds: Duration,
    /// How often the receiver reports what arrived.
    pub interval: Duration,
    /// UDP only: how fast to send, in Mbps; 0 is as fast as the sender can.
    pub mbps: f32,
}

impl Test {
    /// `hello version=1 protocol=tcp direction=upload streams=1 seconds=10 interval_ms=1000 mbps=10`
    pub fn hello(&self) -> String {
        format!(
            "hello version={VERSION} protocol={} direction={} streams={} seconds={} \
             interval_ms={} mbps={}",
            self.protocol.name(),
            self.direction.name(),
            self.streams,
            self.seconds.as_secs_f32(),
            self.interval.as_millis(),
            self.mbps
        )
    }

    pub fn from_hello(line: &str) -> Result<Test, String> {
        let (word, fields) = fields(line);
        if word != "hello" {
            return Err(format!("expected a hello, got {word:?}"));
        }
        let field = |key: &str| {
            fields
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v)
                .ok_or_else(|| format!("the hello has no {key}"))
        };
        let version: u32 = parse(field("version")?)?;
        if version != VERSION {
            return Err(format!(
                "the other end speaks version {version}, this one {VERSION}: update one of them"
            ));
        }
        let protocol = match field("protocol")? {
            "tcp" => Protocol::Tcp,
            "udp" => Protocol::Udp,
            other => return Err(format!("unknown protocol {other}")),
        };
        let direction = match field("direction")? {
            "upload" => Direction::Upload,
            "download" => Direction::Download,
            other => return Err(format!("unknown direction {other}")),
        };
        let seconds: f32 = parse(field("seconds")?)?;
        let interval_ms: u64 = parse(field("interval_ms")?)?;
        let test = Test {
            protocol,
            direction,
            streams: parse(field("streams")?)?,
            seconds: Duration::from_secs_f32(seconds.max(0.1)),
            interval: Duration::from_millis(interval_ms.max(100)),
            mbps: parse(field("mbps")?)?,
        };
        if test.streams == 0 || test.streams > 64 {
            return Err("streams must be 1 to 64".to_string());
        }
        Ok(test)
    }
}

/// What a receiver counted over one interval, or over the whole transfer.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// Seconds since the transfer began, at the interval's start and end.
    pub from: f32,
    pub to: f32,
    pub bytes: u64,
    /// UDP only: the datagrams that arrived, the ones that didn't, and the jitter.
    pub datagrams: Option<Datagrams>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Datagrams {
    pub got: u64,
    pub lost: u64,
    /// How much one datagram's transit differed from the one before, smoothed, in ms.
    pub jitter_ms: f32,
}

impl Report {
    pub fn mbps(&self) -> f32 {
        let seconds = (self.to - self.from) as f64;
        if seconds <= 0.0 {
            return 0.0;
        }
        (self.bytes as f64 * 8.0 / seconds / 1e6) as f32
    }

    /// The share of datagrams lost, as a percentage; None for TCP.
    pub fn loss(&self) -> Option<f32> {
        let datagrams = self.datagrams?;
        Some(match datagrams.got + datagrams.lost {
            0 => 0.0,
            sent => datagrams.lost as f32 * 100.0 / sent as f32,
        })
    }

    /// `interval from=0.000 to=1.000 bytes=117000000` (`total` for the whole transfer), with
    /// `got`, `lost` and `jitter_us` after for UDP.
    pub fn line(&self, total: bool) -> String {
        let mut line = format!(
            "{} from={:.3} to={:.3} bytes={}",
            if total { "total" } else { "interval" },
            self.from,
            self.to,
            self.bytes
        );
        if let Some(datagrams) = self.datagrams {
            line.push_str(&format!(
                " got={} lost={} jitter_us={}",
                datagrams.got,
                datagrams.lost,
                (datagrams.jitter_ms * 1000.0).round() as u64
            ));
        }
        line
    }

    /// Reads a report line back: the report, and whether it was the total.
    pub fn from_line(line: &str) -> Result<(Report, bool), String> {
        let (word, fields) = fields(line);
        let total = match word {
            "interval" => false,
            "total" => true,
            other => return Err(format!("expected a report, got {other:?}")),
        };
        let field = |key: &str| {
            fields
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v)
                .ok_or_else(|| format!("the report has no {key}"))
        };
        let datagrams = match field("got") {
            Ok(got) => {
                let jitter_us: u64 = parse(field("jitter_us")?)?;
                Some(Datagrams {
                    got: parse(got)?,
                    lost: parse(field("lost")?)?,
                    jitter_ms: jitter_us as f32 / 1000.0,
                })
            }
            Err(_) => None,
        };
        let report = Report {
            from: parse(field("from")?)?,
            to: parse(field("to")?)?,
            bytes: parse(field("bytes")?)?,
            datagrams,
        };
        Ok((report, total))
    }
}

/// The first word of a line and its `key=value` pairs.
pub fn fields(line: &str) -> (&str, Vec<(&str, &str)>) {
    let mut words = line.split_whitespace();
    let word = words.next().unwrap_or_default();
    let fields = words.filter_map(|pair| pair.split_once('=')).collect();
    (word, fields)
}

fn parse<T: std::str::FromStr>(value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("can't read {value:?} in a line from the other end"))
}

/// The server's answers to a hello: a token the data connections name, or why not.
pub const OK: &str = "ok";
pub const BUSY: &str = "busy";
pub const REFUSED: &str = "refused";
/// What a data connection (or a UDP socket's first datagram) says: `data token=...`.
pub const DATA: &str = "data";
/// The server's last word on a control connection.
pub const DONE: &str = "done";

/// A UDP datagram: one byte saying what it is, then for data the stream's sequence number and
/// when it was sent (nanoseconds since the sender began), then padding to `DATAGRAM` bytes.
pub const DATAGRAM: usize = 1400;
const HELLO: u8 = 1;
const PACKET: u8 = 2;
const FIN: u8 = 3;

pub enum Datagram<'a> {
    /// Where a stream comes from, by its token: the server sends to the address this came from.
    Hello(&'a str),
    Packet {
        seq: u32,
        sent_ns: u64,
    },
    /// The stream is over; sent a few times, since UDP can lose it.
    Fin,
}

impl Datagram<'_> {
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Datagram::Hello(token) => {
                let mut bytes = vec![HELLO];
                bytes.extend_from_slice(token.as_bytes());
                bytes
            }
            Datagram::Packet { seq, sent_ns } => {
                let mut bytes = vec![0; DATAGRAM];
                bytes[0] = PACKET;
                bytes[1..5].copy_from_slice(&seq.to_be_bytes());
                bytes[5..13].copy_from_slice(&sent_ns.to_be_bytes());
                bytes
            }
            Datagram::Fin => vec![FIN],
        }
    }

    pub fn read(bytes: &[u8]) -> Option<Datagram<'_>> {
        match *bytes.first()? {
            HELLO => Some(Datagram::Hello(std::str::from_utf8(&bytes[1..]).ok()?)),
            PACKET if bytes.len() >= 13 => Some(Datagram::Packet {
                seq: u32::from_be_bytes(bytes[1..5].try_into().ok()?),
                sent_ns: u64::from_be_bytes(bytes[5..13].try_into().ok()?),
            }),
            FIN => Some(Datagram::Fin),
            _ => None,
        }
    }
}

/// Which stream a datagram belongs to: the socket it came in on and where it came from. The
/// server hears every stream on one socket, from a different port each; the client hears one
/// stream a socket.
pub type Stream = (usize, SocketAddr);

#[cfg(test)]
mod tests {
    use super::*;

    fn test() -> Test {
        Test {
            protocol: Protocol::Udp,
            direction: Direction::Download,
            streams: 4,
            seconds: Duration::from_secs(10),
            interval: Duration::from_millis(500),
            mbps: 100.0,
        }
    }

    #[test]
    fn a_hello_reads_back_as_the_test_asked_for() {
        let line = test().hello();
        assert_eq!(
            line,
            "hello version=1 protocol=udp direction=download streams=4 seconds=10 \
             interval_ms=500 mbps=100"
        );
        assert_eq!(Test::from_hello(&line), Ok(test()));
        assert!(Test::from_hello("data token=1").is_err());
        assert!(Test::from_hello("hello version=9 protocol=tcp").is_err());
        assert!(Test::from_hello(&test().hello().replace("streams=4", "streams=0")).is_err());
    }

    #[test]
    fn a_report_reads_back_with_or_without_datagrams() {
        let tcp = Report {
            from: 0.0,
            to: 1.0,
            bytes: 117_000_000,
            datagrams: None,
        };
        assert_eq!(
            tcp.line(false),
            "interval from=0.000 to=1.000 bytes=117000000"
        );
        assert_eq!(
            Report::from_line(&tcp.line(false)),
            Ok((tcp.clone(), false))
        );
        assert_eq!(tcp.mbps(), 936.0);
        assert_eq!(tcp.loss(), None);
        let udp = Report {
            from: 1.0,
            to: 2.0,
            bytes: 1_400_000,
            datagrams: Some(Datagrams {
                got: 990,
                lost: 10,
                jitter_ms: 0.25,
            }),
        };
        assert_eq!(
            udp.line(true),
            "total from=1.000 to=2.000 bytes=1400000 got=990 lost=10 jitter_us=250"
        );
        assert_eq!(Report::from_line(&udp.line(true)), Ok((udp.clone(), true)));
        assert_eq!(udp.loss(), Some(1.0));
        assert!(Report::from_line("done").is_err());
    }

    #[test]
    fn datagrams_read_back() {
        let packet = Datagram::Packet {
            seq: 7,
            sent_ns: 1_500_000,
        };
        let bytes = packet.bytes();
        assert_eq!(bytes.len(), DATAGRAM);
        assert!(matches!(
            Datagram::read(&bytes),
            Some(Datagram::Packet {
                seq: 7,
                sent_ns: 1_500_000
            })
        ));
        assert!(matches!(
            Datagram::read(&Datagram::Hello("abc").bytes()),
            Some(Datagram::Hello("abc"))
        ));
        assert!(matches!(
            Datagram::read(&Datagram::Fin.bytes()),
            Some(Datagram::Fin)
        ));
        assert!(Datagram::read(&[]).is_none());
        assert!(Datagram::read(&[PACKET, 1, 2]).is_none());
    }
}
