//! One question to one name server, over UDP or TCP, timed: the answer as it came back, flags and
//! all, so the screen can say whether it was authoritative.

use super::servers::{self, Server};
use anyhow::Context;
use hickory_proto::op::{Edns, Message, Query};
use hickory_proto::rr::{Name, RData, RecordType};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

/// How long to wait for an answer (twice over UDP, which can lose the question).
const TIMEOUT: Duration = Duration::from_secs(3);

pub struct Lookup {
    pub name: Name,
    pub kind: RecordType,
    pub server: Server,
    /// Over TCP: asked for, or the UDP answer came back cut short.
    pub tcp: bool,
    pub took: Duration,
    pub reply: Result<Message, String>,
}

/// Every lookup asked for: each type from each server, in that order. An address is looked up
/// by its name in in-addr.arpa (or ip6.arpa), PTR by default; a name is A by default. With
/// `authoritative`, the one server asked is the domain's own, found through the first server.
pub fn lookups(
    typed: &str,
    types: &[String],
    servers: &[String],
    authoritative: bool,
    tcp: bool,
) -> anyhow::Result<Vec<Lookup>> {
    let (name, default) = match typed.parse::<IpAddr>() {
        Ok(ip) => (Name::from(ip), RecordType::PTR),
        Err(_) => (
            Name::from_utf8(format!("{}.", typed.trim_end_matches('.')))
                .with_context(|| format!("{typed} is not a name DNS can look up"))?,
            RecordType::A,
        ),
    };
    let kinds = match types {
        [] => vec![default],
        types => types
            .iter()
            .map(|text| {
                text.to_uppercase().parse().map_err(|_| {
                    anyhow::anyhow!("{text} is not a record type, like a, aaaa, mx, txt or ns")
                })
            })
            .collect::<anyhow::Result<_>>()?,
    };
    let mut servers = servers::choose(servers)?;
    if authoritative {
        servers = vec![authority(&name, &servers[0], tcp).map_err(anyhow::Error::msg)?];
    }
    let mut lookups = Vec::new();
    for server in &servers {
        for &kind in &kinds {
            lookups.push(ask(&name, kind, server, tcp));
        }
    }
    Ok(lookups)
}

pub fn ask(name: &Name, kind: RecordType, server: &Server, tcp: bool) -> Lookup {
    let mut query = Message::query();
    query.metadata.recursion_desired = true;
    query.add_query(Query::query(name.clone(), kind));
    // Room for big answers (TXT, DNSKEY) over UDP, the size DNS servers agree on.
    let mut edns = Edns::new();
    edns.set_max_payload(1232);
    query.set_edns(edns);
    let started = Instant::now();
    let mut over_tcp = tcp;
    let mut reply = match query.to_vec() {
        Ok(bytes) if tcp => over_stream(&bytes, server.address),
        Ok(bytes) => over_udp(&bytes, server.address, query.metadata.id),
        Err(error) => Err(error.to_string()),
    };
    if let Ok(answer) = &reply
        && answer.metadata.truncation
    {
        over_tcp = true;
        reply = query
            .to_vec()
            .map_err(|error| error.to_string())
            .and_then(|bytes| over_stream(&bytes, server.address));
    }
    Lookup {
        name: name.clone(),
        kind,
        server: server.clone(),
        tcp: over_tcp,
        took: started.elapsed(),
        reply,
    }
}

fn over_udp(query: &[u8], server: SocketAddr, id: u16) -> Result<Message, String> {
    let local = match server {
        SocketAddr::V4(_) => "0.0.0.0:0",
        SocketAddr::V6(_) => "[::]:0",
    };
    let socket = UdpSocket::bind(local).map_err(words)?;
    socket.connect(server).map_err(words)?;
    socket.set_read_timeout(Some(TIMEOUT)).map_err(words)?;
    let mut buffer = [0; 4096];
    for _ in 0..2 {
        socket.send(query).map_err(words)?;
        // Skip anything that isn't the answer to this question, until the timeout.
        loop {
            match socket.recv(&mut buffer) {
                Ok(size) => match Message::from_vec(&buffer[..size]) {
                    Ok(answer) if answer.metadata.id == id => return Ok(answer),
                    _ => continue,
                },
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break;
                }
                Err(error) => return Err(words(error)),
            }
        }
    }
    Err(format!("no answer in {} s", 2 * TIMEOUT.as_secs()))
}

/// DNS over TCP: each message led by its length in two bytes.
fn over_stream(query: &[u8], server: SocketAddr) -> Result<Message, String> {
    let mut stream = TcpStream::connect_timeout(&server, TIMEOUT).map_err(words)?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(words)?;
    let length = (query.len() as u16).to_be_bytes();
    stream
        .write_all(&[&length, query].concat())
        .map_err(words)?;
    let mut length = [0; 2];
    stream.read_exact(&mut length).map_err(words)?;
    let mut answer = vec![0; u16::from_be_bytes(length) as usize];
    stream.read_exact(&mut answer).map_err(words)?;
    Message::from_vec(&answer).map_err(|error| format!("unreadable answer: {error}"))
}

/// A socket's error as the screen says it.
fn words(error: std::io::Error) -> String {
    match error.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => {
            format!("no answer in {} s", TIMEOUT.as_secs())
        }
        ErrorKind::ConnectionRefused => "refused".to_string(),
        _ => error.to_string(),
    }
}

/// The domain's own name server: the zone's first NS record, looking further up the name until
/// one has them (www.example.com has none; example.com does), and that server's address.
fn authority(name: &Name, via: &Server, tcp: bool) -> Result<Server, String> {
    let mut zone = name.clone();
    loop {
        let answer = ask(&zone, RecordType::NS, via, tcp).reply?;
        let host = answer.answers.iter().find_map(|record| match &record.data {
            RData::NS(ns) if record.name == zone => Some(ns.0.clone()),
            _ => None,
        });
        if let Some(host) = host {
            let ip = ask(&host, RecordType::A, via, tcp)
                .reply?
                .answers
                .iter()
                .find_map(|record| match &record.data {
                    RData::A(a) => Some(a.0),
                    _ => None,
                })
                .ok_or(format!("{host} has no address"))?;
            return Ok(Server {
                address: SocketAddr::new(IpAddr::V4(ip), 53),
                name: Some(host.to_string().trim_end_matches('.').to_string()),
            });
        }
        if zone.is_root() {
            return Err(format!("no name server says it holds {name}"));
        }
        zone = zone.base_name();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::ResponseCode;
    use hickory_proto::rr::Record;
    use hickory_proto::rr::rdata::A;
    use std::net::Ipv4Addr;

    /// A name server on this machine that answers one question with `answer` as authoritative.
    fn stand_in(answer: Ipv4Addr) -> Server {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buffer = [0; 512];
            let (size, from) = socket.recv_from(&mut buffer).unwrap();
            let query = Message::from_vec(&buffer[..size]).unwrap();
            let mut reply = Message::response(query.metadata.id, query.metadata.op_code);
            reply.metadata.authoritative = true;
            let name = query.queries[0].name().clone();
            reply.add_query(query.queries[0].clone());
            reply.add_answer(Record::from_rdata(name, 300, RData::A(A(answer))));
            socket.send_to(&reply.to_vec().unwrap(), from).unwrap();
        });
        Server {
            address,
            name: None,
        }
    }

    #[test]
    fn asks_over_udp_and_reads_the_flags() {
        let server = stand_in(Ipv4Addr::new(192, 168, 1, 20));
        let name = Name::from_utf8("camera.example.com.").unwrap();
        let lookup = ask(&name, RecordType::A, &server, false);
        let reply = lookup.reply.unwrap();
        assert!(reply.metadata.authoritative);
        assert_eq!(reply.metadata.response_code, ResponseCode::NoError);
        assert_eq!(reply.answers[0].ttl, 300);
        assert_eq!(reply.answers[0].data.to_string(), "192.168.1.20");
        assert!(!lookup.tcp);
    }

    #[test]
    fn an_address_is_looked_up_by_its_name() {
        let server = stand_in(Ipv4Addr::new(192, 168, 1, 20));
        let found = lookups(
            "192.168.1.20",
            &[],
            &[server.address.to_string()],
            false,
            false,
        )
        .unwrap();
        assert_eq!(found[0].kind, RecordType::PTR);
        assert_eq!(found[0].name.to_string(), "20.1.168.192.in-addr.arpa.");
        assert!(lookups("example.com", &["nope".to_string()], &[], false, false).is_err());
    }
}
