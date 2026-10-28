//! One address: a UDP poke so the OS asks for its MAC whether or not it answers pings, then a
//! ping for whether it answers and how fast: the system `ping` on Linux and macOS, Windows' own
//! ping call on Windows. All work without root or Administrator.

use crate::ping::echo::Pinger;
use regex::Regex;
use std::net::Ipv4Addr;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

/// The round trip in milliseconds, or None when nothing came back in time.
pub async fn probe(ip: Ipv4Addr, wait: Duration) -> Option<f32> {
    poke(ip).await;
    if cfg!(windows) {
        // Windows' `ping` prints in the system's language (`Zeit=3ms`, `temps=3 ms`) and code
        // page, so there is no reading it everywhere. Its ping call, which the ping mode uses, answers
        // in numbers.
        let echo = tokio::task::spawn_blocking(move || Pinger::new()?.echo(ip, 0, wait));
        return Some(echo.await.ok()?.ok()?.as_secs_f32() * 1000.0);
    }
    let millis = wait.as_millis().to_string();
    let seconds = wait.as_secs_f32().ceil().max(1.0).to_string();
    let mut ping = tokio::process::Command::new("ping");
    if cfg!(target_os = "macos") {
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

/// The `time=1.23 ms` in ping's output.
pub fn latency(output: &str) -> Option<f32> {
    static TIME: OnceLock<Regex> = OnceLock::new();
    let time = TIME.get_or_init(|| {
        Regex::new(r"time=\s*([0-9]+(?:\.[0-9]+)?)\s*ms").expect("the pattern is fixed")
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
        assert_eq!(latency("Request timeout for icmp_seq 0\n"), None);
    }
}
