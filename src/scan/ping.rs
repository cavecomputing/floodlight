//! One address: a UDP poke so the OS asks for its MAC whether or not it answers pings, then the
//! system `ping` for whether it answers and how fast. Both work without root on every OS.

use regex::Regex;
use std::net::Ipv4Addr;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

/// The round trip in milliseconds, or None when nothing came back in time.
pub async fn probe(ip: Ipv4Addr, wait: Duration) -> Option<f32> {
    poke(ip).await;
    let millis = wait.as_millis().to_string();
    let seconds = wait.as_secs_f32().ceil().max(1.0).to_string();
    let mut ping = tokio::process::Command::new("ping");
    if cfg!(windows) {
        ping.args(["-n", "1", "-w", &millis]);
    } else if cfg!(target_os = "macos") {
        // -W: how long to wait for the reply (ms); -t: give up entirely after this many seconds.
        ping.args(["-c", "1", "-W", &millis, "-t", &seconds]);
    } else {
        // Linux's -W is in whole seconds.
        ping.args(["-c", "1", "-W", &seconds]);
    }
    ping.arg(ip.to_string())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(wait + Duration::from_secs(1), ping.output())
        .await
        .ok()?
        .ok()?;
    latency(&String::from_utf8_lossy(&output.stdout))
}

/// A one-byte datagram to the discard port. The host needn't listen: sending it makes the OS
/// resolve the address, and the ARP table keeps the answer.
async fn poke(ip: Ipv4Addr) {
    if let Ok(socket) = tokio::net::UdpSocket::bind("0.0.0.0:0").await {
        let _ = socket.send_to(&[0], (ip, 9)).await;
    }
}

/// The `time=1.23 ms` (or Windows's `time<1ms`) in ping's output.
pub fn latency(output: &str) -> Option<f32> {
    static TIME: OnceLock<Regex> = OnceLock::new();
    let time = TIME.get_or_init(|| {
        Regex::new(r"time[=<]\s*([0-9]+(?:\.[0-9]+)?)\s*ms").expect("the pattern is fixed")
    });
    time.captures(output)?[1].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_round_trip() {
        assert_eq!(
            latency("64 bytes from 192.168.1.1: icmp_seq=0 ttl=64 time=2.122 ms\n"),
            Some(2.122)
        );
        assert_eq!(
            latency("Reply from 192.168.1.1: bytes=32 time<1ms TTL=64"),
            Some(1.0)
        );
        assert_eq!(latency("Request timeout for icmp_seq 0\n"), None);
    }
}
