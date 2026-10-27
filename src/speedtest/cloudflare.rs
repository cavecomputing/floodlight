//! Cloudflare's speed test (speed.cloudflare.com), through the endpoints its own page uses:
//! `__down` sends back as many bytes as asked, `__up` takes what is posted, and each answer's
//! `server-timing` header says how long the server spent, which comes off the time measured here.

use regex::Regex;
use std::io;
use std::time::{Duration, Instant};
use ureq::Agent;

const HOST: &str = "https://speed.cloudflare.com";
/// What a request is taken to have spent on the server when its answer doesn't say, as
/// Cloudflare's own test assumes.
const SERVER_TIME: Duration = Duration::from_millis(10);

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
        .build()
        .into()
}

/// The data center this machine reaches and its provider. `/cdn-cgi/trace`, which every
/// Cloudflare site answers, names the data center; the city and the provider come from the speed
/// test's own `/locations` and `/meta` when they say. None if the trace didn't answer.
pub fn server(agent: &Agent) -> Option<Server> {
    let text = |path: &str| -> Option<String> {
        let mut answer = agent.get(format!("{HOST}{path}")).call().ok()?;
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

/// How long the server says it spent, from a `server-timing` header like
/// `cfRequestDuration;dur=12.3`.
pub fn read_server_time(header: &str) -> Option<Duration> {
    let found = Regex::new(r"dur=([0-9.]+)")
        .expect("a valid pattern")
        .captures(header)?;
    Some(Duration::from_secs_f64(
        found[1].parse::<f64>().ok()? / 1000.0,
    ))
}

/// Fetches `bytes`, and how long it took: from asking to the last byte, less the server's time.
/// With 0 bytes that is the round trip.
pub fn download(agent: &Agent, bytes: usize) -> Result<Duration, String> {
    let started = Instant::now();
    let mut answer = agent
        .get(format!("{HOST}/__down?bytes={bytes}"))
        .call()
        .map_err(words)?;
    let server = server_time(&answer);
    io::copy(&mut answer.body_mut().as_reader(), &mut io::sink()).map_err(|e| e.to_string())?;
    Ok(started.elapsed().saturating_sub(server))
}

/// Sends `body`, and how long it took: from sending to the answer, less the server's time.
pub fn upload(agent: &Agent, body: &[u8]) -> Result<Duration, String> {
    let started = Instant::now();
    let answer = agent
        .post(format!("{HOST}/__up"))
        .send(body)
        .map_err(words)?;
    Ok(started.elapsed().saturating_sub(server_time(&answer)))
}

fn server_time<B>(answer: &ureq::http::Response<B>) -> Duration {
    answer
        .headers()
        .get("server-timing")
        .and_then(|header| header.to_str().ok())
        .and_then(read_server_time)
        .unwrap_or(SERVER_TIME)
}

/// `bytes` moved in `took`, as megabits a second. None for a transfer too quick to time well
/// (under 10 ms, which Cloudflare's own test leaves out too).
pub fn mbps(bytes: usize, took: Duration) -> Option<f32> {
    let seconds = took.as_secs_f64();
    (seconds >= 0.01).then(|| (bytes as f64 * 8.0 / seconds / 1e6) as f32)
}

fn words(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(status) => format!("cloudflare answered {status}"),
        error => error.to_string(),
    }
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
    }

    #[test]
    fn works_out_megabits_a_second() {
        let speed = mbps(10_000_000, Duration::from_millis(800)).expect("timed");
        assert!((speed - 100.0).abs() < 0.01);
        assert_eq!(mbps(100_000, Duration::from_millis(5)), None);
    }
}
