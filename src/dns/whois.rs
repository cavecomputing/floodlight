//! Who registered a domain, or who holds an address, over whois (plain text on TCP port 43):
//! IANA says which registry keeps the name, the registry answers, and for a domain its
//! registrar may know more.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

const IANA: &str = "whois.iana.org";
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct Whois {
    /// What was asked: the registered domain, or the address.
    pub query: String,
    /// The servers that answered, the one that knew the most last.
    pub servers: Vec<String>,
    /// The fields worth reading, each with its values, in the order of `FIELDS`.
    pub fields: Vec<(&'static str, Vec<String>)>,
    /// The last answer's lines, for a registry whose layout `FIELDS` doesn't know.
    pub text: String,
}

/// The fields shown, and the keys each goes by in different registries' answers.
const FIELDS: [(&str, &[&str]); 12] = [
    ("domain", &["domain name", "domain"]),
    (
        "registrar",
        &["registrar", "sponsoring registrar", "registrar name"],
    ),
    (
        "registrant",
        &[
            "registrant organization",
            "registrant",
            "orgname",
            "org-name",
        ],
    ),
    ("network", &["netrange", "inetnum", "inet6num", "cidr"]),
    ("net name", &["netname"]),
    ("country", &["registrant country", "country"]),
    (
        "created",
        &[
            "creation date",
            "created",
            "registered",
            "regdate",
            "registration time",
        ],
    ),
    (
        "updated",
        &[
            "updated date",
            "last updated",
            "last-modified",
            "changed",
            "updated",
        ],
    ),
    (
        "expires",
        &[
            "registry expiry date",
            "registrar registration expiration date",
            "expiry date",
            "expiration date",
            "expires",
            "paid-till",
        ],
    ),
    ("status", &["domain status", "status"]),
    ("name servers", &["name server", "nserver"]),
    ("dnssec", &["dnssec"]),
];

/// The fields that can hold several values; the rest keep their first.
const MANY: [&str; 2] = ["status", "name servers"];

pub fn whois(typed: &str) -> Result<Whois, String> {
    let typed = typed.trim_end_matches('.').to_lowercase();
    let is_address = typed.parse::<IpAddr>().is_ok();
    // IANA is asked about the top-level domain only, or the address.
    let tld = match is_address {
        true => typed.as_str(),
        false => typed.rsplit('.').next().unwrap_or_default(),
    };
    let iana = ask(IANA, tld)?;
    let mut servers = vec![IANA.to_string()];
    let Some(registry) = value(&iana, "refer").or(value(&iana, "whois")) else {
        return Ok(read(typed, servers, &[iana]));
    };
    // The registry knows the registered domain, not www in front of it: drop labels from the
    // left until it does (camera.example.co.uk, example.co.uk, co.uk).
    let mut query = typed.clone();
    let mut text = ask(&registry, &query)?;
    while !is_address && unknown(&text) && query.matches('.').count() > 1 {
        query = query
            .split_once('.')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_default();
        text = ask(&registry, &query)?;
    }
    servers.push(registry.clone());
    let mut answers = vec![text];
    // A thin registry (.com, .net) points at the registrar, who may know the registrant.
    if let Some(registrar) = value(&answers[0], "registrar whois server")
        && registrar != registry
        && let Ok(more) = ask(&registrar, &query)
        && !unknown(&more)
    {
        servers.push(registrar);
        answers.push(more);
    }
    Ok(read(query, servers, &answers))
}

/// The answer to one whois question.
fn ask(server: &str, query: &str) -> Result<String, String> {
    let address = (server, 43)
        .to_socket_addrs()
        .ok()
        .and_then(|mut found| found.next())
        .ok_or(format!("{server} has no address"))?;
    let failed = |error: std::io::Error| format!("{server}: {error}");
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).map_err(failed)?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(failed)?;
    stream
        .write_all(format!("{query}\r\n").as_bytes())
        .map_err(failed)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).map_err(failed)?;
    Ok(String::from_utf8_lossy(&answer).into_owned())
}

/// The first value of `key:` in an answer, without a URL's scheme around a server's name.
fn value(text: &str, key: &str) -> Option<String> {
    pairs(text)
        .find(|(found, _)| found == key)
        .map(|(_, value)| {
            let value = value
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            value.trim_end_matches('/').to_string()
        })
}

/// Each `key: value` line, the key lowercase, lines with no value left out.
fn pairs(text: &str) -> impl Iterator<Item = (String, &str)> {
    text.lines().filter_map(|line| {
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        (!value.is_empty()).then(|| (key.trim().to_lowercase(), value))
    })
}

/// Whether a registry said it has no such name.
fn unknown(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "no match",
        "not found",
        "no data found",
        "no entries found",
        "no object found",
    ]
    .iter()
    .any(|words| text.contains(words))
}

/// The fields from every answer, the registry's first, and the last answer's own lines.
fn read(query: String, servers: Vec<String>, answers: &[String]) -> Whois {
    let text = answers.join("\n");
    let mut fields: Vec<(&'static str, Vec<String>)> = Vec::new();
    for (field, keys) in FIELDS {
        let mut values: Vec<String> = Vec::new();
        for (key, value) in pairs(&text) {
            if !keys.contains(&key.as_str()) {
                continue;
            }
            let value = tidy(field, value);
            if !values
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&value))
            {
                values.push(value);
            }
            if !MANY.contains(&field) {
                break;
            }
        }
        if !values.is_empty() {
            fields.push((field, values));
        }
    }
    // Without its `%` and `#` comments and the legal notice under it.
    let text = answers
        .last()
        .map_or("", String::as_str)
        .lines()
        .take_while(|line| !line.trim_start().starts_with(">>>"))
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with(['%', '#']))
        .collect::<Vec<_>>()
        .join("\n");
    Whois {
        query,
        servers,
        fields,
        text,
    }
}

/// A status without the link after it, a date without its time, a name server in lowercase.
fn tidy(field: &str, value: &str) -> String {
    match field {
        "status" => value.split_whitespace().next().unwrap_or(value).to_string(),
        "created" | "updated" | "expires" if value.get(4..5) == Some("-") => {
            value.split('T').next().unwrap_or(value).to_string()
        }
        "name servers" => value.to_lowercase(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IANA_ANSWER: &str = "% IANA WHOIS server\n\nrefer:        whois.verisign-grs.com\n\n\
                               domain:       COM\norganisation: VeriSign Global Registry Services\n";

    const REGISTRY_ANSWER: &str = "   Domain Name: EXAMPLE.COM\n\
        Registry Domain ID: 2336799_DOMAIN_COM-VRSN\n\
        Registrar WHOIS Server: whois.example-registrar.com\n\
        Updated Date: 2024-08-14T07:01:34Z\n\
        Creation Date: 1995-08-14T04:00:00Z\n\
        Registry Expiry Date: 2025-08-13T04:00:00Z\n\
        Registrar: Example Registrar, Inc.\n\
        Domain Status: clientDeleteProhibited https://icann.org/epp#clientDeleteProhibited\n\
        Domain Status: clientTransferProhibited https://icann.org/epp#clientTransferProhibited\n\
        Name Server: A.IANA-SERVERS.NET\n\
        Name Server: B.IANA-SERVERS.NET\n\
        DNSSEC: signedDelegation\n\
        >>> Last update of whois database: 2024-10-27T12:00:00Z <<<\n\n\
        NOTICE: The expiration date displayed in this record is the date the\n";

    #[test]
    fn follows_iana_to_the_registry() {
        assert_eq!(
            value(IANA_ANSWER, "refer").as_deref(),
            Some("whois.verisign-grs.com")
        );
        assert_eq!(
            value(REGISTRY_ANSWER, "registrar whois server").as_deref(),
            Some("whois.example-registrar.com")
        );
    }

    #[test]
    fn reads_the_fields_worth_showing() {
        let whois = read(
            "example.com".to_string(),
            Vec::new(),
            &[REGISTRY_ANSWER.to_string()],
        );
        let field = |name: &str| {
            whois
                .fields
                .iter()
                .find(|(field, _)| *field == name)
                .map(|(_, values)| values.join(", "))
        };
        assert_eq!(field("domain").as_deref(), Some("EXAMPLE.COM"));
        assert_eq!(
            field("registrar").as_deref(),
            Some("Example Registrar, Inc.")
        );
        assert_eq!(field("created").as_deref(), Some("1995-08-14"));
        assert_eq!(field("expires").as_deref(), Some("2025-08-13"));
        assert_eq!(
            field("status").as_deref(),
            Some("clientDeleteProhibited, clientTransferProhibited")
        );
        assert_eq!(
            field("name servers").as_deref(),
            Some("a.iana-servers.net, b.iana-servers.net")
        );
        assert_eq!(field("dnssec").as_deref(), Some("signedDelegation"));
        assert!(!whois.text.contains("NOTICE"));
    }

    #[test]
    fn reads_an_address_holder() {
        let arin = "NetRange:       192.168.0.0 - 192.168.255.255\nCIDR:           192.168.0.0/16\n\
                    NetName:        PRIVATE-ADDRESS-CBLK-RFC1918-IANA-RESERVED\n\
                    OrgName:        Internet Assigned Numbers Authority\nCountry:        US\n\
                    RegDate:        1994-03-15\n";
        let whois = read("192.168.1.1".to_string(), Vec::new(), &[arin.to_string()]);
        let names: Vec<&str> = whois.fields.iter().map(|(field, _)| *field).collect();
        assert_eq!(
            names,
            ["registrant", "network", "net name", "country", "created"]
        );
        assert_eq!(whois.fields[1].1, ["192.168.0.0 - 192.168.255.255"]);
    }

    #[test]
    fn knows_a_registry_that_has_no_such_name() {
        assert!(unknown("No match for \"WWW.EXAMPLE.COM\".\r\n"));
        assert!(!unknown(REGISTRY_ANSWER));
    }
}
