//! Cloudflare's speed test (speed.cloudflare.com), spoken to through the OS's own `curl`: it ships
//! with Windows 10 and later, macOS and every Linux, so no HTTP or TLS crate is linked in. `curl`
//! reports each transfer's timings, which is all a speed test needs.

use regex::Regex;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const HOST: &str = "https://speed.cloudflare.com";

/// What `curl -w` writes after each transfer: the status, the bytes each way, and the seconds
/// from the start until the request went out, until the first byte came back, and until the end.
const TIMINGS: &str = "%{http_code} %{size_download} %{size_upload} %{time_pretransfer} \
                       %{time_starttransfer} %{time_total}\n";

/// The data center that answered, and the network this machine reached it through.
#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    /// The airport code Cloudflare names its data centers by, like `LAX`.
    pub colo: String,
    pub city: Option<String>,
    /// The internet provider, as its network registers itself.
    pub isp: Option<String>,
}

impl Server {
    /// `LAX · Los Angeles`.
    pub fn place(&self) -> String {
        match &self.city {
            Some(city) => format!("{} · {city}", self.colo),
            None => self.colo.clone(),
        }
    }
}

/// One finished transfer's timings, as `curl` measured them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transfer {
    pub status: u16,
    pub downloaded: u64,
    pub uploaded: u64,
    pub sent: f64,
    pub first_byte: f64,
    pub total: f64,
}

impl Transfer {
    /// One line of `TIMINGS`.
    pub fn parse(line: &str) -> Option<Transfer> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let [status, downloaded, uploaded, sent, first_byte, total] = words[..] else {
            return None;
        };
        Some(Transfer {
            status: status.parse().ok()?,
            downloaded: downloaded.parse().ok()?,
            uploaded: uploaded.parse().ok()?,
            sent: sent.parse().ok()?,
            first_byte: first_byte.parse().ok()?,
            total: total.parse().ok()?,
        })
    }

    /// The round trip of the request: from asking to the first byte of the answer, on a
    /// connection already open.
    pub fn latency_ms(&self) -> f32 {
        ((self.first_byte - self.sent) * 1000.0) as f32
    }

    /// From the request going out to the last byte, the time the bytes took.
    pub fn took(&self) -> Duration {
        Duration::from_secs_f64((self.total - self.sent).max(0.0))
    }

    /// The bytes moved each way, as megabits a second. None for a transfer too quick to time.
    pub fn mbps(&self, bytes: u64) -> Option<f32> {
        let seconds = self.total - self.sent;
        (seconds >= 0.01).then(|| (bytes as f64 * 8.0 / seconds / 1e6) as f32)
    }
}

/// The data center this machine reaches and its provider, from `/meta`'s JSON.
pub async fn server() -> Result<Server, String> {
    let output = Command::new("curl")
        .args(["-sS", &format!("{HOST}/meta")])
        .kill_on_drop(true)
        .output()
        .await
        .map_err(curl_missing)?;
    if !output.status.success() {
        return Err(complaint(&output.stderr));
    }
    read_meta(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "cloudflare didn't say where it answered from".to_string())
}

/// Reads the fields the screen shows from `/meta`. `colo` is an object (`{"iata":"LAX",...}`) on
/// today's endpoint; older answers had it as a plain string, which is read as well.
pub fn read_meta(json: &str) -> Option<Server> {
    let field = |pattern: &str| {
        Regex::new(pattern)
            .expect("a valid pattern")
            .captures(json)
            .map(|found| found[1].to_string())
    };
    let colo = field(r#""colo"\s*:\s*\{[^}]*"iata"\s*:\s*"([^"]+)""#)
        .or_else(|| field(r#""colo"\s*:\s*"([^"]+)""#))?;
    Some(Server {
        colo,
        city: field(r#""colo"\s*:\s*\{[^}]*"city"\s*:\s*"([^"]+)""#),
        isp: field(r#""asOrganization"\s*:\s*"([^"]+)""#),
    })
}

/// Fetches `bytes` from Cloudflare `times` over one connection, handing each transfer over as it
/// ends.
pub async fn download(bytes: u64, times: usize, each: impl FnMut(Transfer)) -> Result<(), String> {
    let url = format!("{HOST}/__down?bytes={bytes}");
    transfers(&url, times, 0, each).await
}

/// Sends `bytes` to Cloudflare `times` over one connection, handing each transfer over as it ends.
pub async fn upload(bytes: usize, times: usize, each: impl FnMut(Transfer)) -> Result<(), String> {
    transfers(&format!("{HOST}/__up"), times, bytes, each).await
}

/// `times` requests to `url` in one `curl`, so the connection is set up once and stays warm. With
/// `upload` bytes, each request posts that many zeros (curl reads them from stdin once and sends
/// them to every URL).
async fn transfers(
    url: &str,
    times: usize,
    upload: usize,
    mut each: impl FnMut(Transfer),
) -> Result<(), String> {
    // Windows has no /dev/null; its name for the same thing is NUL.
    let nowhere = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut command = Command::new("curl");
    command.args(["-sS", "-w", TIMINGS]);
    if upload > 0 {
        command.args(["--data-binary", "@-"]);
    }
    for _ in 0..times {
        command.args(["-o", nowhere, url]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(curl_missing)?;
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            let _ = stdin.write_all(&vec![0; upload]).await;
        });
    }
    let stdout = child.stdout.take().expect("stdout was piped");
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match Transfer::parse(&line) {
            Some(transfer) if transfer.status == 200 => each(transfer),
            // No answer at all: curl's own words on stderr say why, below.
            Some(transfer) if transfer.status == 0 => {}
            Some(transfer) => return Err(format!("cloudflare answered {}", transfer.status)),
            None => {}
        }
    }
    let output = child.wait_with_output().await.map_err(|e| e.to_string())?;
    match output.status.success() {
        true => Ok(()),
        false => Err(complaint(&output.stderr)),
    }
}

fn curl_missing(error: std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "the speed test needs curl, which isn't installed".into(),
        _ => format!("curl: {error}"),
    }
}

/// curl's last line on stderr, like `curl: (6) Could not resolve host: speed.cloudflare.com`.
fn complaint(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    match stderr.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(line) => line.trim().to_string(),
        None => "curl failed without saying why".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_transfers_timings() {
        let transfer =
            Transfer::parse("200 10000000 0 0.050000 0.065000 0.850000").expect("a transfer");
        assert_eq!(transfer.status, 200);
        assert_eq!(transfer.downloaded, 10_000_000);
        assert!((transfer.latency_ms() - 15.0).abs() < 0.01);
        assert!((transfer.mbps(transfer.downloaded).expect("timed") - 100.0).abs() < 0.01);
        assert_eq!(transfer.took(), Duration::from_millis(800));
    }

    #[test]
    fn leaves_out_what_isnt_a_transfer() {
        assert_eq!(Transfer::parse(""), None);
        assert_eq!(Transfer::parse("200 1 2 3"), None);
        assert_eq!(Transfer::parse("ok 1 2 0.1 0.2 0.3"), None);
    }

    #[test]
    fn a_transfer_too_quick_to_time_has_no_speed() {
        let transfer = Transfer::parse("200 0 0 0.010 0.012 0.015").expect("a transfer");
        assert_eq!(transfer.mbps(transfer.downloaded), None);
    }

    #[test]
    fn reads_where_cloudflare_answered_from() {
        let json = r#"{"hostname":"speed.cloudflare.com","clientIp":"192.0.2.1","asn":64500,
            "asOrganization":"Example Fiber","colo":{"iata":"LAX","lat":33.9,"lon":-118.4,
            "cca2":"US","region":"North America","city":"Los Angeles"},"city":"Pasadena"}"#;
        let server = read_meta(json).expect("a server");
        assert_eq!(server.place(), "LAX · Los Angeles");
        assert_eq!(server.isp.as_deref(), Some("Example Fiber"));
    }

    #[test]
    fn reads_the_older_plain_colo() {
        let server = read_meta(r#"{"colo":"AMS","asn":64500}"#).expect("a server");
        assert_eq!(server.place(), "AMS");
        assert_eq!(server.isp, None);
        assert_eq!(read_meta("{}"), None);
    }
}
