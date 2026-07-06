//! The archive profile's custody verbs: `intake` and `audit`.
//!
//! Part II of `docs/detached-bodies-and-archive-profile.1.0.md`: custody and
//! browsing are different things. Custody is content-addressed and boring —
//! attestations live in `store/` under their id, bodies live in `bodies/`
//! under their blake3 — and browsing is a regenerable `views/` tree built
//! *from* custody, safe to delete at any time.
//!
//! The preservation stance is normative here: a body whose hash no longer
//! matches is a state to be judged, not a sentence to be executed. Nothing in
//! this module deletes, refuses to export, or silently repairs bytes — drift
//! is marked and reported, and the bytes stay.
//!
//! Layout (relative to the archive root, whose door is `<root>/.comms/`):
//!
//! ```text
//! store/    attestations, <id>.cbor            — ONE store
//! bodies/   detached bodies, <b3-hex>[.<ext>]  — ext advisory, for human mercy
//! intake/   drop zone for session bundles
//! views/    regenerable, human-named; never the archive
//! genesis/  frozen as ratified; intake never touches it
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;

use crate::bundle::{
    author_general_claim, content_report, inspect_bundle, parse_attestation, parse_bundle,
    BodyStatus, Bundle, ClaimSpec, ContentReport,
};
use crate::cbor::Value;
use crate::now_rfc3339;
use crate::steward::Attestation;

/// The fixed custody directories beside the archive's `.comms/` door.
pub struct Archive {
    pub root: PathBuf,
}

impl Archive {
    pub fn at(root: &Path) -> Archive {
        Archive {
            root: root.to_path_buf(),
        }
    }

    pub fn store(&self) -> PathBuf {
        self.root.join("store")
    }
    pub fn bodies(&self) -> PathBuf {
        self.root.join("bodies")
    }
    pub fn views(&self) -> PathBuf {
        self.root.join("views")
    }

    /// The on-disk body file for a blake3 hex, whatever advisory extension it
    /// was stored with. None when the body is not in custody.
    pub fn body_file(&self, b3_hex: &str) -> Option<PathBuf> {
        let dir = self.bodies();
        let entries = std::fs::read_dir(&dir).ok()?;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == b3_hex || name.starts_with(&format!("{b3_hex}.")) {
                return Some(e.path());
            }
        }
        None
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Advisory file extension for a media type — for human mercy in `bodies/`
/// and `views/`; never load-bearing (the hash is the name).
pub fn ext_for(media_type: &str) -> &'static str {
    let mt = media_type.split(';').next().unwrap_or("");
    match mt.trim() {
        "text/markdown" => ".md",
        "application/json" => ".json",
        "application/cbor" => ".cbor",
        "text/html" => ".html",
        m if m.starts_with("text/") => ".txt",
        _ => "",
    }
}

/// The media key (`z<base58(blake3)>`) decoded back to the 32-byte hash.
fn key_to_hash(key: &str) -> Option<[u8; 32]> {
    let b = bs58::decode(key.strip_prefix('z')?).into_vec().ok()?;
    <[u8; 32]>::try_from(b.as_slice()).ok()
}

#[derive(Debug, Default)]
pub struct IntakeReport {
    pub new_members: Vec<String>,
    pub kept_members: usize,
    pub new_bodies: Vec<String>,
    pub kept_bodies: usize,
    pub views_regenerated: Vec<String>,
    pub custody_attestation: Option<String>,
}

impl IntakeReport {
    pub fn is_noop(&self) -> bool {
        self.new_members.is_empty() && self.new_bodies.is_empty()
    }
}

/// Intake a sealed session bundle: verify → ingest → route → attest custody.
///
/// Refuses (whole, not partially) unless the bundle's A1.8 seal verifies,
/// every member's signatures hold, no member's content is malformed, and
/// every media blob matches its content key. Re-intake of an already-ingested
/// bundle is a no-op: nothing is duplicated and no custody attestation is
/// written for nothing.
pub fn intake_bundle(
    archive: &Archive,
    bundle_path: &Path,
    custodian: &SigningKey,
) -> Result<IntakeReport, String> {
    let bytes =
        std::fs::read(bundle_path).map_err(|e| format!("{}: {e}", bundle_path.display()))?;
    let bundle = parse_bundle(&bytes).map_err(|e| format!("{}: {e}", bundle_path.display()))?;
    let inspect = inspect_bundle(&bundle);

    // Verify: seal, member signatures, content shape, media keys. Body
    // `absent` is fine (commitments may outrun the media at hand); a
    // `mismatched` blob inside an arriving bundle is refused — custody takes
    // what the seal vouches for, and these bytes contradict their own name.
    if inspect.seal.sealed_by.is_none() {
        return Err(
            "bundle carries no A1.8 seal — intake takes sealed close bundles \
                    (unattested material goes through --legacy)"
                .to_owned(),
        );
    }
    if !inspect.seal.ok {
        return Err(format!(
            "bundle seal FAILS (missing: {:?}, extra: {:?}) — refusing intake",
            inspect.seal.missing, inspect.seal.extra
        ));
    }
    for m in &inspect.members {
        if !m.all_signatures_ok {
            return Err(format!(
                "member {} has invalid signatures — refusing intake",
                m.id
            ));
        }
        if let ContentReport::Malformed(why) = &m.content {
            return Err(format!(
                "member {} content malformed: {why} — refusing intake",
                m.id
            ));
        }
    }
    for (key, ok) in &inspect.media {
        if !ok {
            return Err(format!(
                "media blob {key} does not match its content key — refusing intake \
                 (bytes already in custody are never touched; fix the bundle)"
            ));
        }
    }

    let mut report = IntakeReport::default();

    // Ingest members by id (idempotent by construction: the id is the name).
    let store = archive.store();
    std::fs::create_dir_all(&store).map_err(|e| format!("{}: {e}", store.display()))?;
    for att in &bundle.attestations {
        let id = att.id();
        let path = store.join(format!("{id}.cbor"));
        if path.exists() {
            report.kept_members += 1;
        } else {
            std::fs::write(&path, att.to_cbor()).map_err(|e| format!("{}: {e}", path.display()))?;
            report.new_members.push(id);
        }
    }

    // Ingest media by hash. The advisory extension comes from whichever
    // member committed to these bytes, when one did.
    let bodies = archive.bodies();
    std::fs::create_dir_all(&bodies).map_err(|e| format!("{}: {e}", bodies.display()))?;
    let mut media_types: HashMap<String, String> = HashMap::new();
    for att in &bundle.attestations {
        if let ContentReport::Detached { body_b3, .. } = content_report(att, &bundle.media) {
            if let Some(mt) = att
                .core
                .get("c")
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("media_type"))
                .and_then(Value::as_text)
            {
                media_types.insert(hex(&body_b3), mt.to_owned());
            }
        }
    }
    for (key, blob) in &bundle.media {
        let Some(hash) = key_to_hash(key) else {
            return Err(format!("media key {key} does not decode — refusing intake"));
        };
        let b3_hex = hex(&hash);
        if archive.body_file(&b3_hex).is_some() {
            report.kept_bodies += 1;
            continue;
        }
        let ext = media_types.get(&b3_hex).map(|mt| ext_for(mt)).unwrap_or("");
        let path = bodies.join(format!("{b3_hex}{ext}"));
        std::fs::write(&path, blob).map_err(|e| format!("{}: {e}", path.display()))?;
        report.new_bodies.push(b3_hex);
    }

    // Route: regenerate views for each session (author steward) present.
    report.views_regenerated = regenerate_views(archive, &bundle)?;

    // Attest custody: one custodian-signed record naming everything ingested.
    // A no-op intake leaves no record to duplicate.
    if !report.is_noop() {
        let seal_id = inspect
            .members
            .iter()
            .find(|m| m.is_seal)
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let mut body = String::new();
        body.push_str(&format!(
            "# custody: intake of {}\n\nseal: {seal_id}\n\n",
            bundle_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
        body.push_str(&format!(
            "members ingested ({}):\n",
            report.new_members.len()
        ));
        for id in &report.new_members {
            body.push_str(&format!("- {id}\n"));
        }
        body.push_str(&format!(
            "\nbodies ingested ({}):\n",
            report.new_bodies.len()
        ));
        for h in &report.new_bodies {
            body.push_str(&format!("- blake3 {h}\n"));
        }
        let support = [seal_id];
        let id = write_custody(archive, custodian, "custody-intake", &body, &support)?;
        report.custody_attestation = Some(id);
    }
    Ok(report)
}

/// Intake unattested material as legacy testimony (II.5): ingested by hash,
/// custody-attested with whatever provenance the custodian can honestly
/// state. Nothing is discarded; nothing is promoted to attested either.
pub fn intake_legacy(
    archive: &Archive,
    source: &Path,
    custodian: &SigningKey,
    provenance: &str,
) -> Result<IntakeReport, String> {
    let mut files = Vec::new();
    collect_files(source, &mut files)?;
    if files.is_empty() {
        return Err(format!("{}: no files to intake", source.display()));
    }

    let bodies = archive.bodies();
    std::fs::create_dir_all(&bodies).map_err(|e| format!("{}: {e}", bodies.display()))?;
    let mut report = IntakeReport::default();
    let mut listing = String::new();
    for f in &files {
        let blob = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let b3_hex = hex(blake3::hash(&blob).as_bytes());
        let kept = archive.body_file(&b3_hex).is_some();
        if kept {
            report.kept_bodies += 1;
        } else {
            // Keep the original extension as the advisory one.
            let ext = f
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let path = bodies.join(format!("{b3_hex}{ext}"));
            std::fs::write(&path, &blob).map_err(|e| format!("{}: {e}", path.display()))?;
            report.new_bodies.push(b3_hex.clone());
        }
        listing.push_str(&format!(
            "- {} -> blake3 {b3_hex}{}\n",
            f.display(),
            if kept { " (already in custody)" } else { "" }
        ));
    }

    if !report.is_noop() {
        let body = format!(
            "# custody: legacy testimony\n\nprovenance, as honestly as the custodian \
             can state it:\n\n{provenance}\n\nfiles ({}):\n{listing}",
            files.len()
        );
        let id = write_custody(archive, custodian, "legacy-testimony", &body, &[])?;
        report.custody_attestation = Some(id);
    }
    Ok(report)
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for e in entries {
        collect_files(&e, out)?;
    }
    Ok(())
}

/// Author + sign a custody attestation into the archive's own store. The
/// archive's history is itself attested.
fn write_custody(
    archive: &Archive,
    custodian: &SigningKey,
    kind: &str,
    body: &str,
    support: &[String],
) -> Result<String, String> {
    let now = now_rfc3339();
    let spec = ClaimSpec {
        about: "archive",
        kind,
        body: body.as_bytes(),
        media_type: "text/markdown",
        detach: false,
        support,
        refs: &[],
        language: "zxx",
        community: None,
        occasion: Some("archive custody"),
        issued_at: &now,
    };
    let att = author_general_claim(&spec, custodian, "custodian", &now);
    let id = att.id();
    let path = archive.store().join(format!("{id}.cbor"));
    std::fs::write(&path, att.to_cbor()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(id)
}

/// Rebuild `views/sessions/<session-tag>/` for every session whose artifacts
/// this bundle carries. Views are copies of custody bytes with human names —
/// deleting and rebuilding them is always safe, and this does exactly that
/// for the affected sessions only.
fn regenerate_views(archive: &Archive, bundle: &Bundle) -> Result<Vec<String>, String> {
    // Group general-claim members by their author steward (the session key).
    let mut by_session: BTreeMap<String, Vec<&Attestation>> = BTreeMap::new();
    for att in &bundle.attestations {
        let Some(by) = att.signatures.first().map(|s| s.by.clone()) else {
            continue;
        };
        let tag: String = by
            .strip_prefix("comms.steward:")
            .unwrap_or(&by)
            .chars()
            .take(16)
            .collect();
        by_session.entry(tag).or_default().push(att);
    }

    let mut regenerated = Vec::new();
    for (tag, atts) in by_session {
        let dir = archive.views().join("sessions").join(&tag);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for att in atts {
            let Some(claim) = att.core.get("c") else {
                continue;
            };
            if claim.get("t").and_then(Value::as_text) != Some("general-claim/1") {
                continue;
            }
            let about = claim
                .get("about")
                .and_then(Value::as_text)
                .unwrap_or("artifact");
            let Some(content) = claim.get("content") else {
                continue;
            };
            let mt = content
                .get("media_type")
                .and_then(Value::as_text)
                .unwrap_or("");
            let id = att.id();
            let tail: String = id
                .chars()
                .rev()
                .take(8)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let name = format!("{}.{}{}", sanitize(about), tail, ext_for(mt));

            // Embedded body: the bytes are in the claim. Detached: copy from
            // custody when present; an absent body simply leaves no view file
            // (the commitment is still browsable in store/).
            let bytes: Option<Vec<u8>> =
                if let Some(b) = content.get("body").and_then(Value::as_bytes) {
                    Some(b.to_vec())
                } else if let Some(b3) = content.get("body_b3").and_then(Value::as_bytes) {
                    archive
                        .body_file(&hex(b3))
                        .and_then(|p| std::fs::read(p).ok())
                } else {
                    None
                };
            if let Some(bytes) = bytes {
                let path = dir.join(name);
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        regenerated.push(tag);
    }
    Ok(regenerated)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

// ---- audit ------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AuditReport {
    /// store/: attestations whose bytes still derive their filename id and
    /// whose signatures verify.
    pub store_intact: usize,
    /// store/: (path, why) — unparseable, misnamed, or signature-failing.
    /// Marked, never deleted.
    pub store_drift: Vec<(String, String)>,
    /// bodies/: files whose content still hashes to their name.
    pub bodies_intact: usize,
    /// bodies/: (path, why) — content no longer matches the name. Retained.
    pub bodies_drift: Vec<(String, String)>,
    /// Detached commitments in store with no body in custody. The normal
    /// state for material never granted to the archive; listed for judgment.
    pub bodies_absent: Vec<(String, String)>,
    /// Bodies no store attestation commits to (legacy testimony, or orphans).
    pub bodies_unreferenced: usize,
}

impl AuditReport {
    pub fn drift(&self) -> bool {
        !self.store_drift.is_empty() || !self.bodies_drift.is_empty()
    }
}

/// Re-derive every id and hash in custody; mark drift, never delete (the
/// preservation stance). Views are not audited — they are regenerable by
/// definition.
pub fn audit(archive: &Archive) -> Result<AuditReport, String> {
    let mut report = AuditReport::default();

    // bodies/: content must hash to the filename stem.
    let mut body_hashes: Vec<String> = Vec::new();
    let bodies_dir = archive.bodies();
    if bodies_dir.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&bodies_dir)
            .map_err(|e| format!("{}: {e}", bodies_dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for p in entries {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let stem = name.split('.').next().unwrap_or("").to_owned();
            let Ok(blob) = std::fs::read(&p) else {
                report.bodies_drift.push((name, "unreadable".to_owned()));
                continue;
            };
            let actual = hex(blake3::hash(&blob).as_bytes());
            if actual == stem {
                report.bodies_intact += 1;
            } else {
                report.bodies_drift.push((
                    name,
                    format!("content hashes to {actual}, not its name — retained"),
                ));
            }
            body_hashes.push(stem);
        }
    }

    // store/: bytes must parse, derive their filename id, and verify. The
    // store is inspected as one pseudo-bundle over the bodies at hand, which
    // also judges every detached commitment's body status.
    let store_dir = archive.store();
    let mut attestations = Vec::new();
    let mut referenced: Vec<String> = Vec::new();
    if store_dir.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&store_dir)
            .map_err(|e| format!("{}: {e}", store_dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "cbor").unwrap_or(false))
            .collect();
        entries.sort();
        for p in entries {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let Ok(bytes) = std::fs::read(&p) else {
                report.store_drift.push((name, "unreadable".to_owned()));
                continue;
            };
            let att = match parse_attestation(&bytes) {
                Ok(a) => a,
                Err(e) => {
                    report
                        .store_drift
                        .push((name, format!("does not parse: {e} — retained")));
                    continue;
                }
            };
            let id = att.id();
            let stem = name.strip_suffix(".cbor").unwrap_or(&name);
            if stem != id {
                report.store_drift.push((
                    name.clone(),
                    format!("bytes derive {id}, not their name — retained"),
                ));
                continue;
            }
            attestations.push(att);
        }
    }

    // One media map from bodies/, keyed by each file's *claimed* hash (its
    // name) so drifted bytes surface as `mismatched` on their attestation.
    let mut media = HashMap::new();
    for h in &body_hashes {
        if let Some(p) = archive.body_file(h) {
            if let (Ok(blob), Some(hash)) = (std::fs::read(&p), hex_to_hash(h)) {
                media.insert(format!("z{}", bs58::encode(hash).into_string()), blob);
            }
        }
    }

    for att in &attestations {
        let id = att.id();
        let mut ok = true;
        for sig in &att.signatures {
            if sig.alg != "ed25519" {
                continue; // community signatures need their keyset chain; out of scope here
            }
            let Some(pk) = crate::bundle::pubkey_from_steward_id(&sig.by) else {
                ok = false;
                continue;
            };
            let Ok(raw) = <[u8; 64]>::try_from(sig.signature.as_slice()) else {
                ok = false;
                continue;
            };
            if !crate::personal_verify(&att.core, &sig.by, &sig.role, &sig.signed_at, &pk, &raw) {
                ok = false;
            }
        }
        if !ok {
            report.store_drift.push((
                format!("{id}.cbor"),
                "signature verification fails — retained".to_owned(),
            ));
            continue;
        }
        match content_report(att, &media) {
            ContentReport::Malformed(why) => {
                report
                    .store_drift
                    .push((format!("{id}.cbor"), format!("{why} — retained")));
            }
            ContentReport::Detached {
                body_b3, status, ..
            } => {
                let h = hex(&body_b3);
                match status {
                    BodyStatus::Verified => {
                        referenced.push(h);
                        report.store_intact += 1;
                    }
                    BodyStatus::Absent => {
                        report.store_intact += 1;
                        report.bodies_absent.push((id, h));
                    }
                    BodyStatus::Mismatched => {
                        referenced.push(h);
                        report.store_intact += 1; // the attestation itself is intact
                    }
                }
            }
            _ => report.store_intact += 1,
        }
    }

    report.bodies_unreferenced = body_hashes
        .iter()
        .filter(|h| !referenced.contains(h))
        .count();
    Ok(report)
}

fn hex_to_hash(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// The bundle media key for a raw blake3 hash — for callers resolving
/// deliveries by hash.
pub fn key_for_hash(hash: &[u8; 32]) -> String {
    format!("z{}", bs58::encode(hash).into_string())
}

// ---- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{make_bundle, media_key};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("comms-archive-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for d in [
            "store",
            "bodies",
            "views/sessions",
            "views/keys",
            "intake",
            "genesis",
        ] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        dir
    }

    fn key(byte: u8) -> SigningKey {
        SigningKey::from_bytes(&[byte; 32])
    }

    fn detached_member(body: &[u8], about: &str, sk: &SigningKey) -> Attestation {
        let spec = ClaimSpec {
            about,
            kind: "testimony",
            body,
            media_type: "text/markdown",
            detach: true,
            support: &[],
            refs: &[],
            language: "zxx",
            community: None,
            occasion: Some("test"),
            issued_at: "2026-07-06T00:00:00Z",
        };
        author_general_claim(&spec, sk, "author", "2026-07-06T00:00:01Z")
    }

    fn sealed_bundle(body: &[u8]) -> Bundle {
        let member = detached_member(body, "letter/session-test", &key(1));
        let mut media = HashMap::new();
        media.insert(media_key(body), body.to_vec());
        make_bundle(
            vec![member],
            media,
            Some(&key(2)),
            "test close bundle",
            "2026-07-06T00:00:02Z",
            "2026-07-06T00:00:03Z",
            "2026-07-06T00:00:04Z",
        )
    }

    #[test]
    fn intake_bundle_ingests_by_id_and_hash_then_noops() {
        let root = scratch("intake");
        let archive = Archive::at(&root);
        let bundle = sealed_bundle(b"# letter\nbody\n");
        let bundle_path = root.join("intake").join("close.bundle");
        std::fs::write(&bundle_path, bundle.to_cbor()).unwrap();

        let report = intake_bundle(&archive, &bundle_path, &key(3)).unwrap();
        assert_eq!(report.new_members.len(), 2, "member plus seal enter store");
        assert_eq!(report.new_bodies.len(), 1);
        assert!(report.custody_attestation.is_some());
        let body_hash = report.new_bodies[0].clone();
        assert!(archive.body_file(&body_hash).unwrap().is_file());
        assert!(!report.views_regenerated.is_empty());
        assert!(
            std::fs::read_dir(archive.views().join("sessions"))
                .unwrap()
                .any(|e| e.unwrap().path().is_dir()),
            "intake should regenerate a session view"
        );

        let again = intake_bundle(&archive, &bundle_path, &key(3)).unwrap();
        assert!(again.is_noop(), "same bundle should not duplicate custody");
        assert!(again.custody_attestation.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn intake_refuses_unsealed_or_bad_media_without_partial_ingest() {
        let root = scratch("refuse");
        let archive = Archive::at(&root);
        let body = b"body";
        let member = detached_member(body, "letter", &key(1));
        let unsealed = Bundle {
            attestations: vec![member.clone()],
            media: HashMap::new(),
            manifest: None,
        };
        let unsealed_path = root.join("intake").join("unsealed.bundle");
        std::fs::write(&unsealed_path, unsealed.to_cbor()).unwrap();
        let err = intake_bundle(&archive, &unsealed_path, &key(3)).unwrap_err();
        assert!(err.contains("no A1.8 seal"), "{err}");
        assert!(std::fs::read_dir(archive.store()).unwrap().next().is_none());

        let mut bad_media = HashMap::new();
        bad_media.insert(
            "z11111111111111111111111111111111".to_owned(),
            b"not that hash".to_vec(),
        );
        let bad = make_bundle(
            vec![member],
            bad_media,
            Some(&key(2)),
            "bad media",
            "2026-07-06T00:00:02Z",
            "2026-07-06T00:00:03Z",
            "2026-07-06T00:00:04Z",
        );
        let bad_path = root.join("intake").join("bad.bundle");
        std::fs::write(&bad_path, bad.to_cbor()).unwrap();
        let err = intake_bundle(&archive, &bad_path, &key(3)).unwrap_err();
        assert!(err.contains("media blob"), "{err}");
        assert!(std::fs::read_dir(archive.store()).unwrap().next().is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_reports_absent_and_drift_without_deleting_bytes() {
        let root = scratch("audit");
        let archive = Archive::at(&root);
        let body = b"keep me even when corrupted";
        let bundle = sealed_bundle(body);
        let bundle_path = root.join("intake").join("close.bundle");
        std::fs::write(&bundle_path, bundle.to_cbor()).unwrap();
        let report = intake_bundle(&archive, &bundle_path, &key(3)).unwrap();
        let h = report.new_bodies[0].clone();

        let clean = audit(&archive).unwrap();
        assert_eq!(clean.bodies_drift.len(), 0);
        assert_eq!(clean.bodies_absent.len(), 0);

        let body_path = archive.body_file(&h).unwrap();
        std::fs::write(&body_path, b"corrupted but retained").unwrap();
        let drift = audit(&archive).unwrap();
        assert_eq!(drift.bodies_drift.len(), 1);
        assert!(body_path.is_file(), "audit must retain mismatched bytes");

        std::fs::remove_file(&body_path).unwrap();
        let absent = audit(&archive).unwrap();
        assert!(absent.bodies_absent.iter().any(|(_, hash)| hash == &h));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_intake_hashes_testimony_and_attests_once() {
        let root = scratch("legacy");
        let archive = Archive::at(&root);
        let source = root.join("legacy-input");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("note.txt"), b"found in the shoebox").unwrap();

        let report = intake_legacy(&archive, &source, &key(3), "found in old contarchive").unwrap();
        assert_eq!(report.new_bodies.len(), 1);
        assert!(report.custody_attestation.is_some());
        assert!(archive.body_file(&report.new_bodies[0]).unwrap().is_file());

        let again = intake_legacy(&archive, &source, &key(3), "found in old contarchive").unwrap();
        assert!(again.is_noop());
        assert!(again.custody_attestation.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }
}
