//! The network's public (WAN) address, as the internet sees it: Cloudflare's `/cdn-cgi/trace`
//! asked at 1.1.1.1 for IPv4 and at its IPv6 address for IPv6, so each family is asked over
//! itself, with ipify as the fallback; the provider and the place from the speed test's `/meta`.
//! No UI.

use crate::brand;
use crate::speedtest::cloudflare;
use std::net::IpAddr;
use std::time::Duration;
use ureq::Agent;

/// Where each family is asked, the first that answers winning.
const V4: [&str; 2] = ["https://1.1.1.1/cdn-cgi/trace", "https://api.ipify.org"];
const V6: [&str; 2] = [
    "https://[2606:4700:4700::1111]/cdn-cgi/trace",
    "https://api6.ipify.org",
];
/// What Cloudflare knows about the address asking: its provider, city and country.
const META: &str = "https://speed.cloudflare.com/meta";

pub struct Wan {
    /// The public IPv4 address, or why none came back.
    pub v4: Result<IpAddr, String>,
    /// The public IPv6 address, or why none came back (often: the network has none).
    pub v6: Result<IpAddr, String>,
    /// The internet provider, as its network registers itself.
    pub isp: Option<String>,
    /// `Los Angeles · US`.
    pub place: Option<String>,
}

/// Asks for both families and the provider at once, so a family that doesn't answer doesn't hold
/// up the others.
pub fn find() -> Wan {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .user_agent(format!("{}/{}", brand::BIN, env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    std::thread::scope(|threads| {
        let v4 = threads.spawn(|| address(&agent, &V4));
        let v6 = threads.spawn(|| address(&agent, &V6));
        let meta = get(&agent, META).unwrap_or_default();
        let field = |name: &str| cloudflare::field(&meta, name);
        let place = match (field("city"), field("country")) {
            (Some(city), Some(country)) => Some(format!("{city} · {country}")),
            (city, country) => city.or(country),
        };
        Wan {
            v4: v4.join().expect("the IPv4 lookup doesn't panic"),
            v6: v6.join().expect("the IPv6 lookup doesn't panic"),
            isp: field("asOrganization"),
            place,
        }
    })
}

/// The address the first of `urls` to answer saw; the last one's failure if none did.
fn address(agent: &Agent, urls: &[&str]) -> Result<IpAddr, String> {
    let mut why = String::new();
    for url in urls {
        match get(agent, url).map(|body| read_address(&body)) {
            Ok(Some(address)) => return Ok(address),
            Ok(None) => why = format!("{url} answered without an address"),
            Err(error) => why = error,
        }
    }
    Err(why)
}

fn get(agent: &Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        // `/meta` refuses (403) a request that doesn't come from the speed test's page.
        .header("referer", "https://speed.cloudflare.com/")
        .call()
        .and_then(|mut answer| answer.body_mut().read_to_string())
        .map_err(|error| error.to_string())
}

/// The trace's `ip=` line, or ipify's whole answer, which is the address alone.
fn read_address(body: &str) -> Option<IpAddr> {
    let ip = body.lines().find_map(|line| line.strip_prefix("ip="));
    ip.unwrap_or(body).trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_address_from_either_answer() {
        let trace = "fl=1f1\nh=1.1.1.1\nip=192.0.2.1\nts=1761570000.1\ncolo=LAX\nloc=US\n";
        assert_eq!(read_address(trace), "192.0.2.1".parse().ok());
        assert_eq!(read_address("2001:db8::1\n"), "2001:db8::1".parse().ok());
        assert_eq!(read_address("<html>blocked</html>"), None);
    }
}
