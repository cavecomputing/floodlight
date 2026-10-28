//! Cloudflare's speed test (speed.cloudflare.com), through the endpoints its own page uses:
//! `__down` sends back as many bytes as asked, `__up` takes what is posted, and each answer's
//! `server-timing` header says how long the server spent, which comes off the time measured here.

use crate::brand;
use regex::Regex;
use std::io;
use std::time::{Duration, Instant};
use ureq::http::Response;
use ureq::{Agent, Body};

const HOST: &str = "https://speed.cloudflare.com";
/// How many times a request told to slow down (429) is asked again, as Cloudflare's page does.
const RETRIES: u32 = 3;
/// How a request's failure starts when Cloudflare's firewall turned it away.
pub const REFUSED: &str = "cloudflare answered 403";
/// How it starts when Cloudflare said too many (429) and to come back later than is worth waiting.
pub const LIMITED: &str = "cloudflare answered 429";
/// The longest a 429's wait is waited out. Once a machine has fetched a lot in a while Cloudflare
/// asks it to wait 3600 s, which would leave the test silent for an hour.
const LONGEST_WAIT: Duration = Duration::from_secs(10);

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

/// Requests that share their connections, so each after the first skips the setup, and hand
/// back the cookies Cloudflare sets, like a browser's.
pub fn agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        // Statuses are read here, so a 429's retry-after can be.
        .http_status_as_error(false)
        // Our own name rather than the HTTP library's, so Cloudflare sees what is asking.
        .user_agent(format!("{}/{}", brand::BIN, env!("CARGO_PKG_VERSION")))
        // Keeps a connection for each of the split transfers; the library keeps 3, so one had
        // to be set up again, inside the timing, every round.
        .max_idle_connections_per_host(super::runs::STREAMS)
        .build()
        .into()
}

/// The data center this machine reaches and its provider. `/cdn-cgi/trace`, which every
/// Cloudflare site answers, names the data center; the city and the provider come from the speed
/// test's own `/locations` and `/meta` when they say. None if the trace didn't answer.
pub fn server(agent: &Agent) -> Option<Server> {
    let text = |path: &str| -> Option<String> {
        let mut answer = agent
            .get(format!("{HOST}{path}"))
            // `/locations` and `/meta` refuse (403) a request that doesn't come from the page.
            .header("referer", format!("{HOST}/"))
            .call()
            .ok()?;
        if !answer.status().is_success() {
            return None;
        }
        answer.body_mut().read_to_string().ok()
    };
    let colo = read_trace(&text("/cdn-cgi/trace")?)?;
    let city = text("/locations").and_then(|json| read_city(&json, &colo));
    let isp = text("/meta").and_then(|json| field(&json, "asOrganization"));
    Some(Server { colo, city, isp })
}

/// `colo=LAX` from the trace's `key=value` lines.
pub fn read_trace(trace: &str) -> Option<String> {
    trace
        .lines()
        .find_map(|line| line.strip_prefix("colo="))
        .map(|colo| colo.trim().to_string())
        .filter(|colo| !colo.is_empty())
}

/// The city of the data center `colo` in `/locations`' list, one `{...}` object each.
pub fn read_city(json: &str, colo: &str) -> Option<String> {
    json.split('}')
        .find(|place| field(place, "iata").as_deref() == Some(colo))
        .and_then(|place| field(place, "city"))
}

/// A string field's value in a piece of JSON, like `"city":"Los Angeles"`.
fn field(json: &str, name: &str) -> Option<String> {
    Regex::new(&format!(r#""{name}"\s*:\s*"([^"]+)""#))
        .expect("a valid pattern")
        .captures(json)
        .map(|found| found[1].to_string())
}

/// How long the server says it spent, from its `server-timing` headers: `cfRequestDuration;dur=12.3`
/// where it gives one, otherwise its parts added up (`cfSpeedEdge;dur=4, cfSpeedWorker;dur=20`),
/// as Cloudflare's page reads them.
pub fn read_server_time(header: &str) -> Option<Duration> {
    let total = |name: &str| -> f64 {
        Regex::new(&format!(r"(?i)(?:^|,\s*){name};\s*dur=([0-9.]+)"))
            .expect("a valid pattern")
            .captures_iter(header)
            .filter_map(|found| found[1].parse::<f64>().ok())
            .sum()
    };
    let request = total("cfReq(?:uest)?Dur(?:ation)?");
    let ms = if request > 0.01 {
        request
    } else {
        total("cfSpeed[a-z]*")
    };
    (ms > 0.01).then(|| Duration::from_secs_f64(ms / 1000.0))
}

/// Fetches `bytes`, and how long it took: from asking to the last byte, less the server's time.
/// With 0 bytes that is the round trip.
pub fn download(agent: &Agent, bytes: usize) -> Result<Duration, String> {
    let url = format!("{HOST}/__down?bytes={bytes}");
    let (started, mut answer) = answered(|| agent.get(&url).call())?;
    let server = server_time(&answer);
    io::copy(&mut answer.body_mut().as_reader(), &mut io::sink()).map_err(|e| e.to_string())?;
    Ok(started.elapsed().saturating_sub(server))
}

/// Sends `body`, and how long it took: from sending to the answer. Cloudflare's page counts the
/// server's time in here, so this does too. The size goes in the URL as well, and the body as
/// text, both as the page sends them.
pub fn upload(agent: &Agent, body: &[u8]) -> Result<Duration, String> {
    let url = format!("{HOST}/__up?bytes={}", body.len());
    let (started, _) = answered(|| {
        agent
            .post(&url)
            .header("content-type", "text/plain;charset=UTF-8")
            // Where the page's own uploads come from, as a browser says.
            .header("origin", HOST)
            .header("referer", format!("{HOST}/"))
            .send(body)
    })?;
    Ok(started.elapsed())
}

/// Makes the request, waiting and asking again when Cloudflare says too many (429), as often as
/// its page does, unless it asks for a longer wait than `LONGEST_WAIT`. Hands back when the
/// answer's request went out, and the answer.
fn answered(
    request: impl Fn() -> Result<Response<Body>, ureq::Error>,
) -> Result<(Instant, Response<Body>), String> {
    let mut retries = 0;
    loop {
        let started = Instant::now();
        let mut answer = request().map_err(|e| e.to_string())?;
        let status = answer.status().as_u16();
        let wait = retry_after(&answer);
        if status == 429 && retries < RETRIES && wait <= LONGEST_WAIT {
            retries += 1;
            std::thread::sleep(wait);
            continue;
        }
        if !answer.status().is_success() {
            return Err(refusal(&mut answer));
        }
        return Ok((started, answer));
    }
}

/// `cloudflare answered 403 · challenge · ray 8d1f...`: the status, and what Cloudflare's own
/// headers say about why (`cf-mitigated` names the firewall action, `cf-ray` the request, for
/// looking the refusal up), with the first words of the body when it is text.
fn refusal(answer: &mut Response<Body>) -> String {
    let status = answer.status().as_u16();
    let mut words = format!("cloudflare answered {status}");
    if status == 429 {
        words.push_str(" · too many tests from this machine lately, try again in a few minutes");
    }
    let header = |name: &str| -> Option<String> {
        let value = answer.headers().get(name)?.to_str().ok()?;
        Some(value.to_string())
    };
    if let Some(action) = header("cf-mitigated") {
        words.push_str(&format!(" · {action}"));
    }
    if let Some(ray) = header("cf-ray") {
        words.push_str(&format!(" · ray {ray}"));
    }
    let body = answer.body_mut().read_to_string().unwrap_or_default();
    if let Some(line) = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('<'))
    {
        let line: String = line.chars().take(60).collect();
        words.push_str(&format!(" · {line}"));
    }
    words
}

/// How long a 429 asks to wait, in seconds; 5 when it doesn't say, as Cloudflare's page assumes.
fn retry_after(answer: &Response<Body>) -> Duration {
    let seconds = answer
        .headers()
        .get("retry-after")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.trim().parse().ok())
        .unwrap_or(5);
    Duration::from_secs(seconds)
}

/// The server's time, from every `server-timing` header it sent (there can be more than one).
fn server_time(answer: &Response<Body>) -> Duration {
    let headers: Vec<&str> = answer
        .headers()
        .get_all("server-timing")
        .iter()
        .filter_map(|header| header.to_str().ok())
        .collect();
    read_server_time(&headers.join(", ")).unwrap_or_default()
}

/// `bytes` moved in `took`, as megabits a second, with the 0.5% Cloudflare's page adds for the
/// headers and framing around them. None for a transfer too quick to time well (under 10 ms,
/// which the page leaves out too).
pub fn mbps(bytes: usize, took: Duration) -> Option<f32> {
    let seconds = took.as_secs_f64();
    (seconds >= 0.01).then(|| (bytes as f64 * 1.005 * 8.0 / seconds / 1e6) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_data_center_from_the_trace() {
        let trace = "fl=1f1\nh=speed.cloudflare.com\nip=192.0.2.1\nts=1761570000.1\n\
                     visit_scheme=https\ncolo=LAX\nhttp=http/2\nloc=US\ntls=TLSv1.3\n";
        assert_eq!(read_trace(trace).as_deref(), Some("LAX"));
        assert_eq!(read_trace("fl=1f1\ncolo=\n"), None);
    }

    #[test]
    fn finds_the_data_centers_city() {
        let json = r#"[{"iata":"AMS","lat":52.3,"lon":4.8,"cca2":"NL","region":"Europe",
            "city":"Amsterdam"},{"iata":"LAX","lat":33.9,"lon":-118.4,"cca2":"US",
            "region":"North America","city":"Los Angeles"}]"#;
        assert_eq!(read_city(json, "LAX").as_deref(), Some("Los Angeles"));
        assert_eq!(read_city(json, "SJC"), None);
    }

    #[test]
    fn reads_the_provider() {
        let json = r#"{"clientIp":"192.0.2.1","asn":64500,"asOrganization":"Example Fiber"}"#;
        assert_eq!(
            field(json, "asOrganization").as_deref(),
            Some("Example Fiber")
        );
    }

    #[test]
    fn reads_the_servers_time() {
        let header = "cfRequestDuration;dur=12.5";
        assert_eq!(
            read_server_time(header),
            Some(Duration::from_micros(12_500))
        );
        assert_eq!(read_server_time("cfL4;desc=\"?proto=TCP\""), None);
        let parts = "cfSpeedEdge;dur=4, cfSpeedWorker;dur=20, cfL4;desc=\"?proto=TCP&rtt=16867\"";
        assert_eq!(read_server_time(parts), Some(Duration::from_millis(24)));
    }

    #[test]
    fn works_out_megabits_a_second() {
        let speed = mbps(10_000_000, Duration::from_millis(804)).expect("timed");
        assert!((speed - 100.0).abs() < 0.01);
        assert_eq!(mbps(100_000, Duration::from_millis(5)), None);
    }
}
