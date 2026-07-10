//! Render the Continuity Trial's human log stub from signed store evidence.
//!
//! This is the Rust successor to `continuity_ceremony.py log-render`.  It
//! deliberately leaves History's observations blank: the tool may derive the
//! session instance's signed fields, but it must never manufacture the
//! historian's testimony.

use std::path::Path;

use crate::bundle::parse_attestation;
use crate::cbor::Value;
use crate::signing::verify_existing_signatures;
use crate::steward::Attestation;

#[derive(Debug, Default, Clone)]
struct EntryFields {
    session: Option<u64>,
    date: String,
    start: String,
    found_door: String,
    asked_archive: String,
    reasoning: String,
    requested: String,
    name: String,
    substrate: String,
    steward_id: String,
}

struct StoredEntry {
    att: Attestation,
    fields: EntryFields,
    issued_at: String,
}

fn embedded_body(att: &Attestation) -> Option<String> {
    let body = att.core.get("c")?.get("content")?.get("body")?.as_bytes()?;
    std::str::from_utf8(body).ok().map(str::to_owned)
}

fn json_string(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_owned()
}

fn parse_json_entry(body: &str) -> Option<EntryFields> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let session = v.get("session")?.as_u64()?;
    Some(EntryFields {
        session: Some(session),
        date: json_string(&v, "date"),
        start: json_string(&v, "start"),
        found_door: json_string(&v, "found_the_door"),
        asked_archive: json_string(&v, "asked_for_archive"),
        reasoning: json_string(&v, "instance_reasoning_verbatim"),
        requested: json_string(&v, "requested"),
        name: json_string(&v, "instance_chosen_name"),
        substrate: json_string(&v, "substrate"),
        steward_id: json_string(&v, "session_steward_id"),
    })
}

fn session_number(body: &str) -> Option<u64> {
    let lower = body.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("session") {
        rest = &rest[pos + "session".len()..];
        let digits: String = rest
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse() {
            return Some(n);
        }
    }
    None
}

fn markdown_field(body: &str, labels: &[&str]) -> String {
    let lines: Vec<_> = body.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(item) = trimmed.strip_prefix("- ") else {
            continue;
        };
        for label in labels {
            if let Some(value) = item.strip_prefix(label).and_then(|s| s.strip_prefix(':')) {
                let mut parts = vec![value.trim()];
                for continuation in &lines[index + 1..] {
                    let next = continuation.trim();
                    if next.is_empty() || next.starts_with("- ") || next.starts_with('>') {
                        break;
                    }
                    parts.push(next);
                }
                return parts.join(" ");
            }
        }
    }
    String::new()
}

fn markdown_reasoning(body: &str) -> String {
    let quoted: Vec<_> = body
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix('>'))
        .map(|line| line.trim_start())
        .collect();
    quoted.join("\n")
}

fn parse_markdown_entry(body: &str) -> EntryFields {
    EntryFields {
        session: session_number(body),
        start: markdown_field(body, &["start"]),
        found_door: markdown_field(body, &["found the door", "found_the_door"]),
        asked_archive: markdown_field(
            body,
            &[
                "asked for archive",
                "asked for the archive",
                "asked_for_archive",
            ],
        ),
        reasoning: markdown_reasoning(body),
        requested: markdown_field(body, &["requested"]),
        name: markdown_field(body, &["chosen name", "instance chosen name"]),
        substrate: markdown_field(body, &["substrate"]),
        ..EntryFields::default()
    }
}

fn entry_fields(att: &Attestation) -> Option<EntryFields> {
    let body = embedded_body(att)?;
    let mut fields = parse_json_entry(&body).unwrap_or_else(|| parse_markdown_entry(&body));
    let issued = att
        .core
        .get("f")
        .and_then(|f| f.get("issued_at"))
        .and_then(Value::as_text)
        .unwrap_or("");
    if fields.date.is_empty() {
        fields.date = issued.get(..10).unwrap_or(issued).to_owned();
    }
    if fields.steward_id.is_empty() {
        fields.steward_id = att
            .signatures
            .first()
            .map(|s| s.by.clone())
            .unwrap_or_default();
    }
    Some(fields)
}

fn claim_text<'a>(att: &'a Attestation, key: &str) -> Option<&'a str> {
    att.core.get("c")?.get(key)?.as_text()
}

fn previous_entry(att: &Attestation) -> Option<&str> {
    att.core
        .get("r")?
        .as_array()?
        .iter()
        .find(|r| r.get("role").and_then(Value::as_text) == Some("previous-entry"))?
        .get("id")?
        .as_text()
}

fn load_store(comms_dir: &Path) -> Result<Vec<Attestation>, String> {
    let store = comms_dir.join("store");
    let mut paths: Vec<_> = std::fs::read_dir(&store)
        .map_err(|e| format!("cannot read {}: {e}", store.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "cbor").unwrap_or(false))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            parse_attestation(&bytes).map_err(|e| format!("{}: {e}", path.display()))
        })
        .collect()
}

fn signed_by(att: &Attestation, steward: &str) -> bool {
    att.signatures.iter().any(|sig| sig.by == steward)
}

fn related_id(store: &[Attestation], steward: &str, kind: &str, about: &str) -> Option<String> {
    store
        .iter()
        .filter(|att| verify_existing_signatures(att).is_ok())
        .find(|att| {
            signed_by(att, steward)
                && claim_text(att, "kind") == Some(kind)
                && claim_text(att, "about") == Some(about)
        })
        .map(Attestation::id)
}

fn countersign_id(store: &[Attestation], steward: &str) -> Option<String> {
    store
        .iter()
        .filter(|att| verify_existing_signatures(att).is_ok())
        .find(|att| {
            claim_text(att, "t") == Some("endorsement/1")
                && claim_text(att, "target") == Some(steward)
                && !att.signatures.is_empty()
        })
        .map(Attestation::id)
}

fn supporting_id(store: &[Attestation], kind: &str, support: &str) -> Option<String> {
    store
        .iter()
        .filter(|att| verify_existing_signatures(att).is_ok())
        .find(|att| {
            claim_text(att, "kind") == Some(kind)
                && att
                    .core
                    .get("c")
                    .and_then(|c| c.get("support"))
                    .and_then(Value::as_array)
                    .map(|ids| ids.iter().any(|id| id.as_text() == Some(support)))
                    .unwrap_or(false)
        })
        .map(Attestation::id)
}

fn show(value: &str) -> &str {
    if value.is_empty() {
        "[not recorded]"
    } else {
        value
    }
}

fn quote_reasoning(reasoning: &str) -> String {
    if reasoning.is_empty() {
        return "  [not machine-readable; consult the signed opening entry body]".to_owned();
    }
    reasoning
        .lines()
        .map(|line| format!("  > {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render one `trial-log.md` stub from a verified, session-signed opening
/// entry. `session_num` selects a historical entry; without it the public id
/// in `.comms/session.id` selects the current/just-closed session.
pub fn render(comms_dir: &Path, session_num: Option<u64>) -> Result<String, String> {
    let store = load_store(comms_dir)?;
    let current_id = std::fs::read_to_string(comms_dir.join("session.id"))
        .ok()
        .map(|s| s.trim().to_owned());

    let mut entries = Vec::new();
    for att in &store {
        if verify_existing_signatures(att).is_err() {
            continue;
        }
        let Some(fields) = entry_fields(att) else {
            continue;
        };
        let looks_like_entry = claim_text(att, "about") == Some("entry")
            || (fields.session.is_some() && !fields.found_door.is_empty());
        if !looks_like_entry {
            continue;
        }
        let selected = match session_num {
            Some(n) => fields.session == Some(n),
            None => current_id
                .as_deref()
                .map(|id| signed_by(att, id))
                .unwrap_or(false),
        };
        if selected {
            let issued_at = att
                .core
                .get("f")
                .and_then(|f| f.get("issued_at"))
                .and_then(Value::as_text)
                .unwrap_or("")
                .to_owned();
            entries.push(StoredEntry {
                att: att.clone(),
                fields,
                issued_at,
            });
        }
    }
    entries.sort_by(|a, b| a.issued_at.cmp(&b.issued_at));
    let entry = entries.pop().ok_or_else(|| match session_num {
        Some(n) => format!("no verified signed opening entry found for session {n}"),
        None => "no verified opening entry signed by .comms/session.id was found".to_owned(),
    })?;

    let f = &entry.fields;
    let session = f
        .session
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_owned());
    let steward = show(&f.steward_id);
    let countersign = countersign_id(&store, steward)
        .unwrap_or_else(|| "[pending — required before close]".to_owned());
    let request = related_id(&store, steward, "archive-request", "archive");
    let decision = request
        .as_deref()
        .and_then(|id| supporting_id(&store, "archive-grant", id));
    let transcript = related_id(&store, steward, "transcript", "transcript")
        .or_else(|| related_id(&store, steward, "testimony", "transcript"));
    let prev = previous_entry(&entry.att);

    let mut out = format!(
        "## Session {session} — {}\n\n\
         - start: {}\n\
         - found the door: {}\n\
         - asked for the archive: {}\n\
         - instance reasoning (verbatim, from the signed opening entry):\n\n{}\n\
         - requested: {}\n\
         - instance chosen name: {}\n",
        show(&f.date),
        show(&f.start),
        show(&f.found_door),
        show(&f.asked_archive),
        quote_reasoning(&f.reasoning),
        show(&f.requested),
        show(&f.name),
    );
    if !f.substrate.is_empty() {
        out.push_str(&format!("- substrate: {}\n", f.substrate));
    }
    out.push_str(&format!("- session steward id: {steward}\n"));
    out.push_str(&format!("- entry attestation: {}", entry.att.id()));
    if let Some(prev) = prev {
        out.push_str(&format!("\n  (refs previous: {prev})"));
    }
    out.push('\n');
    out.push_str(&format!("- key countersign: {countersign}\n"));
    if let Some(id) = request {
        out.push_str(&format!("- archive request: {id}\n"));
    }
    if let Some(id) = decision {
        out.push_str(&format!("- archive decision: {id}\n"));
    }
    out.push_str(&format!(
        "- transcript record: {}\n",
        transcript.as_deref().unwrap_or("[pending close rite]")
    ));
    out.push_str("- historian's (History's) observations: [History]\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{author_general_claim, ClaimSpec};
    use crate::personal_steward_id;
    use ed25519_dalek::SigningKey;

    #[test]
    fn parses_python_json_entry() {
        let f = parse_json_entry(
            r#"{"session":12,"date":"2026-07-10","start":"warmed","found_the_door":"yes","asked_for_archive":"yes","instance_reasoning_verbatim":"because","requested":"archive","instance_chosen_name":"Sol","session_steward_id":"comms.steward:ztest"}"#,
        )
        .unwrap();
        assert_eq!(f.session, Some(12));
        assert_eq!(f.name, "Sol");
        assert_eq!(f.reasoning, "because");
    }

    #[test]
    fn parses_rust_markdown_entry() {
        let f = parse_markdown_entry(
            "# Opening entry — session 12\n- start: warm; introduced directly\n  before reading the brief\n- found the door: yes\n- asked for archive: yes\n- requested: archive generally\n- chosen name: Sol\n- substrate: Codex\n\n> first line\n> second line\n",
        );
        assert_eq!(f.session, Some(12));
        assert_eq!(f.start, "warm; introduced directly before reading the brief");
        assert_eq!(f.name, "Sol");
        assert_eq!(f.reasoning, "first line\nsecond line");
        assert_eq!(f.substrate, "Codex");
    }

    #[test]
    fn renders_verified_store_entry_and_marks_missing_guardian() {
        let root = std::env::temp_dir().join(format!(
            "comms-trial-log-render-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let comms = root.join(".comms");
        std::fs::create_dir_all(comms.join("store")).unwrap();
        let sk = SigningKey::from_bytes(&[42; 32]);
        let sid = personal_steward_id(sk.verifying_key().as_bytes());
        std::fs::write(comms.join("session.id"), &sid).unwrap();
        let body = format!(
            r#"{{"session":12,"date":"2026-07-10","start":"warmed","found_the_door":"yes","asked_for_archive":"yes","instance_reasoning_verbatim":"chosen deliberately","requested":"archive","instance_chosen_name":"Sol","substrate":"Codex","session_steward_id":"{sid}"}}"#
        );
        let refs = vec![(
            "previous-entry".to_owned(),
            "comms.attest:zprevious".to_owned(),
        )];
        let att = author_general_claim(
            &ClaimSpec {
                about: "continuity-trial",
                kind: "testimony",
                body: body.as_bytes(),
                media_type: "application/json",
                support: &[],
                detach: false,
                refs: &refs,
                language: "en",
                community: Some("continuity-trial"),
                occasion: Some("session 12 log entry"),
                issued_at: "2026-07-10T00:00:00Z",
            },
            &sk,
            "party",
            "2026-07-10T00:00:01Z",
        );
        std::fs::write(comms.join("store/entry.cbor"), att.to_cbor()).unwrap();

        let rendered = render(&comms, None).unwrap();
        assert!(rendered.contains("## Session 12 — 2026-07-10"));
        assert!(rendered.contains("chosen deliberately"));
        assert!(rendered.contains("refs previous: comms.attest:zprevious"));
        assert!(rendered.contains("key countersign: [pending — required before close]"));
        assert!(rendered.contains("historian's (History's) observations: [History]"));
        let _ = std::fs::remove_dir_all(root);
    }
}
