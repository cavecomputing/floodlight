//! The lookup as a report: the name and how the first answer went up top, a sentence on what it
//! means, then a table of records for each lookup, and the whois fields.

use super::sheet::{self, COOL, Cell, Column, FG, MUTED, Sheet};
use crate::dns::lookup::Lookup;
use crate::dns::whois::Whois;
use crate::ui::dns;
use hickory_proto::rr::{RData, Record};
use std::path::Path;

const RECORDS: [Column; 4] = [
    Column::mono("name", 0.3),
    Column::number("ttl", 0.08),
    Column::text("type", 0.1),
    Column::mono("data", 0.52),
];

const FIELDS: [Column; 2] = [Column::text("field", 0.2), Column::text("value", 0.8)];

pub fn write(
    path: &Path,
    lookups: &[Lookup],
    whois: Option<&Result<Whois, String>>,
) -> anyhow::Result<()> {
    std::fs::write(path, document(lookups, whois))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub fn document(lookups: &[Lookup], whois: Option<&Result<Whois, String>>) -> Vec<u8> {
    let (date, time) = sheet::now();
    let mut sheet = Sheet::new(format!("DNS lookup · {date} {time}"));
    let first = &lookups[0];
    let name = first.name.to_string();
    let name = name.trim_end_matches('.');
    let mut kinds: Vec<String> = lookups
        .iter()
        .map(|lookup| lookup.kind.to_string())
        .collect();
    kinds.dedup();
    let (verdict, _) = dns::verdict(first);
    let facts = [
        ("date", date),
        ("time", time),
        ("type", kinds.join(", ")),
        ("over", if first.tcp { "TCP" } else { "UDP" }.to_string()),
        (
            "answer",
            format!("{:.1} ms", first.took.as_secs_f32() * 1000.0),
        ),
    ];
    let subtitle = format!("from {}", first.server.label());
    sheet.masthead("DNS lookup", name, &subtitle, &facts);
    sheet.summary(&format!(
        "Asked {} for {name}'s {} records over {}: the answer took {:.1} ms, {}. {}",
        first.server.label(),
        first.kind,
        if first.tcp { "TCP" } else { "UDP" },
        first.took.as_secs_f32() * 1000.0,
        verdict.replace(" · ", ", "),
        summary(lookups)
    ));

    for lookup in lookups {
        let title = format!("{} from {}", lookup.kind, lookup.server.label());
        sheet.heading(&title, 36.0);
        let (verdict, _) = dns::verdict(lookup);
        let (answers, authorities) = match &lookup.reply {
            Ok(reply) => (&reply.answers[..], &reply.authorities[..]),
            Err(_) => (&[][..], &[][..]),
        };
        let row = |record: &Record| {
            let [owner, ttl, kind, data] = dns::record_words(record);
            let ink = match record.data {
                RData::A(_) | RData::AAAA(_) => COOL,
                _ => FG,
            };
            vec![(owner, FG), (ttl, MUTED), (kind, MUTED), (data, ink)]
        };
        let mut rows: Vec<Vec<Cell>> = answers.iter().map(row).collect();
        // No records: say why, above the zone's SOA that came with it.
        if rows.is_empty() {
            rows.push(vec![
                (name.to_string(), FG),
                ("—".to_string(), MUTED),
                (lookup.kind.to_string().to_lowercase(), MUTED),
                (verdict, MUTED),
            ]);
        }
        rows.extend(authorities.iter().map(row));
        sheet.table(&RECORDS, &rows);
    }

    match whois {
        Some(Ok(whois)) => {
            sheet.heading(&format!("Whois · {}", whois.servers.join(" · ")), 36.0);
            let rows: Vec<Vec<Cell>> = match whois.fields.is_empty() {
                true => whois
                    .text
                    .lines()
                    .map(|line| vec![(String::new(), MUTED), (line.to_string(), FG)])
                    .collect(),
                false => whois
                    .fields
                    .iter()
                    .flat_map(|(field, values)| {
                        values.iter().enumerate().map(|(i, value)| {
                            let label = if i == 0 {
                                field.to_string()
                            } else {
                                String::new()
                            };
                            vec![(label, MUTED), (value.clone(), FG)]
                        })
                    })
                    .collect(),
            };
            sheet.table(&FIELDS, &rows);
        }
        Some(Err(why)) => {
            sheet.heading("Whois", 36.0);
            sheet.table(
                &FIELDS,
                &[vec![(String::new(), MUTED), (format!("failed: {why}"), FG)]],
            );
        }
        None => {}
    }
    sheet.finish()
}

/// What an authoritative answer means, or what a non-authoritative one does.
fn summary(lookups: &[Lookup]) -> String {
    let authoritative = lookups
        .iter()
        .any(|lookup| matches!(&lookup.reply, Ok(reply) if reply.metadata.authoritative));
    match authoritative {
        true => "An authoritative answer comes from a name server that holds the domain's own \
                 records, so it is what the domain says right now."
            .to_string(),
        false => "A non-authoritative answer comes from a resolver that asked the domain's own \
                  name servers, or answered from what it remembered: a change made in the last \
                  ttl may not show yet."
            .to_string(),
    }
}
