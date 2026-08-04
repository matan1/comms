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

use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    /// Members already in custody that the incoming copy added signers to:
    /// `(attestation id, signatures gained)`. Custody grew without a new id.
    pub merged_signatures: Vec<(String, usize)>,
    pub new_bodies: Vec<String>,
    pub kept_bodies: usize,
    pub views_regenerated: Vec<String>,
    pub custody_attestation: Option<String>,
}

impl IntakeReport {
    pub fn is_noop(&self) -> bool {
        self.new_members.is_empty()
            && self.new_bodies.is_empty()
            && self.merged_signatures.is_empty()
    }
}

/// Do every signature on this attestation verify, judged on its own terms?
/// Used before a merged copy replaces what custody already holds.
fn verify_member_signatures(att: &Attestation) -> Result<(), String> {
    let pseudo = Bundle {
        attestations: vec![att.clone()],
        media: HashMap::new(),
        manifest: None,
    };
    let ok = inspect_bundle(&pseudo)
        .members
        .first()
        .map(|m| m.all_signatures_ok)
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err("a signature does not verify".to_owned())
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
        // A legacy store may hold the same attestation under its bare
        // multibase id; both names carry the same bytes — do not duplicate.
        let bare = id.strip_prefix("comms.attest:").unwrap_or(&id);
        let legacy = store.join(format!("{bare}.cbor"));
        let held = [&path, &legacy].into_iter().find(|p| p.exists()).cloned();
        match held {
            // Equal ids are the same core, not necessarily the same witness
            // set. Keeping custody's copy and discarding the incoming one
            // loses every signer custody had not seen — an arriving bundle
            // can carry the richer variant. Merge instead of coalesce.
            Some(existing_path) => {
                let bytes = std::fs::read(&existing_path)
                    .map_err(|e| format!("{}: {e}", existing_path.display()))?;
                let mut existing = parse_attestation(&bytes)
                    .map_err(|e| format!("{}: {e}", existing_path.display()))?;
                let added = crate::signing::merge_new_signatures(&mut existing, att);
                if added > 0 {
                    // The merged copy must stand on its own before it replaces
                    // what custody holds; a bad signer takes nothing in.
                    verify_member_signatures(&existing).map_err(|e| {
                        format!(
                            "{id}: merging {added} incoming signature(s) into custody: {e} \
                             — custody is left untouched"
                        )
                    })?;
                    std::fs::write(&existing_path, existing.to_cbor())
                        .map_err(|e| format!("{}: {e}", existing_path.display()))?;
                    report.merged_signatures.push((id, added));
                } else {
                    report.kept_members += 1;
                }
            }
            None => {
                std::fs::write(&path, att.to_cbor())
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                report.new_members.push(id);
            }
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
        if !report.merged_signatures.is_empty() {
            // Custody changed without a new id: say so explicitly, or the
            // record reads as if nothing arrived for these members.
            body.push_str(&format!(
                "\nsignatures merged into members already in custody ({}):\n",
                report.merged_signatures.len()
            ));
            for (id, n) in &report.merged_signatures {
                body.push_str(&format!("- {id}: +{n} signature(s)\n"));
            }
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
            // Legacy stores name files by the bare multibase id, without the
            // `comms.attest:` scheme prefix; both forms name the same bytes.
            let bare = id.strip_prefix("comms.attest:").unwrap_or(&id);
            if stem != id && stem != bare {
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
            ContentReport::LegacyDetached { body_hash, status } => {
                let h = hex(&body_hash);
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

// ---- catalog ---------------------------------------------------------------

/// Read-only inventory for an archive-shaped or legacy directory. Unlike
/// `audit`, this makes no claims about custody layout or attestation validity;
/// it describes the bytes present so a viewer can decide what to inspect next.
#[derive(Debug, Default)]
pub struct CatalogReport {
    pub root: String,
    pub files: usize,
    pub bytes: u64,
    pub text_files: usize,
    pub text_lines: u64,
    pub symlinks_skipped: usize,
    pub categories: BTreeMap<String, CatalogCategory>,
    pub kinds: BTreeMap<String, usize>,
    pub entries: Vec<CatalogEntry>,
    pub duplicates: Vec<DuplicateGroup>,
    pub duplicate_bytes: u64,
}

#[derive(Debug, Default)]
pub struct CatalogCategory {
    pub files: usize,
    pub bytes: u64,
    pub text_files: usize,
    pub text_lines: u64,
}

#[derive(Debug)]
pub struct CatalogEntry {
    pub path: String,
    pub category: String,
    pub bytes: u64,
    pub blake3: String,
    pub kind: String,
    pub lines: Option<u64>,
}

#[derive(Debug)]
pub struct DuplicateGroup {
    pub blake3: String,
    pub bytes_each: u64,
    pub paths: Vec<String>,
}

/// Hash and classify every regular file below `root`, following no symlinks
/// and writing nothing. Paths and maps are sorted for deterministic output.
pub fn catalog(root: &Path) -> Result<CatalogReport, String> {
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let canonical = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    let mut paths = Vec::new();
    let mut symlinks_skipped = 0;
    collect_regular_files(&canonical, &mut paths, &mut symlinks_skipped)?;
    paths.sort();

    let mut report = CatalogReport {
        root: canonical.to_string_lossy().into_owned(),
        symlinks_skipped,
        ..CatalogReport::default()
    };
    let mut by_hash: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();

    for path in paths {
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rel = path.strip_prefix(&canonical).map_err(|e| format!("{}: {e}", path.display()))?;
        let rel_text = rel.to_string_lossy().replace('\\', "/");
        let mut components = rel.components();
        let first = components.next();
        let category = if components.next().is_none() {
            "(root)".to_owned()
        } else {
            first.map(|c| c.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_else(|| "(root)".to_owned())
        };
        let kind = path.extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "(none)".to_owned());
        let size = data.len() as u64;
        let digest = hex(blake3::hash(&data).as_bytes());
        let lines = std::str::from_utf8(&data).ok().filter(|_| !data.contains(&0)).map(|_| {
            if data.is_empty() { 0 } else {
                data.iter().filter(|b| **b == b'\n').count() as u64
                    + u64::from(data.last() != Some(&b'\n'))
            }
        });

        report.files += 1;
        report.bytes += size;
        *report.kinds.entry(kind.clone()).or_default() += 1;
        let summary = report.categories.entry(category.clone()).or_default();
        summary.files += 1;
        summary.bytes += size;
        if let Some(n) = lines {
            report.text_files += 1;
            report.text_lines += n;
            summary.text_files += 1;
            summary.text_lines += n;
        }
        by_hash.entry(digest.clone()).or_default().push((rel_text.clone(), size));
        report.entries.push(CatalogEntry {
            path: rel_text,
            category,
            bytes: size,
            blake3: digest,
            kind,
            lines,
        });
    }

    for (digest, members) in by_hash {
        if members.len() < 2 { continue; }
        let size = members[0].1;
        report.duplicate_bytes += size * (members.len() as u64 - 1);
        report.duplicates.push(DuplicateGroup {
            blake3: digest,
            bytes_each: size,
            paths: members.into_iter().map(|(path, _)| path).collect(),
        });
    }
    Ok(report)
}

fn collect_regular_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    symlinks_skipped: &mut usize,
) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let ty = entry.file_type().map_err(|e| format!("{}: {e}", entry.path().display()))?;
        if ty.is_symlink() {
            *symlinks_skipped += 1;
        } else if ty.is_dir() {
            collect_regular_files(&entry.path(), files, symlinks_skipped)?;
        } else if ty.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

// ---- manifest --------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestLevel {
    Minimal,
    Full,
}

impl ManifestLevel {
    pub fn parse(s: &str) -> Result<ManifestLevel, String> {
        match s {
            "minimal" => Ok(ManifestLevel::Minimal),
            "full" => Ok(ManifestLevel::Full),
            _ => Err(format!("manifest level must be minimal or full (got '{s}')")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ManifestLevel::Minimal => "minimal",
            ManifestLevel::Full => "full",
        }
    }
}

/// Generate a deterministic archive view. The snapshot id commits to the full
/// sorted inventory even when the minimal projection withholds its entries.
pub fn manifest(root: &Path, level: ManifestLevel) -> Result<serde_json::Value, String> {
    let report = manifest_catalog(root)?;
    let snapshot_id = inventory_snapshot_id(&report.entries);
    let sessions = discover_sessions(&report.entries);

    let categories: serde_json::Map<String, serde_json::Value> = report
        .categories
        .iter()
        .map(|(name, s)| {
            (
                name.clone(),
                serde_json::json!({
                    "files": s.files,
                    "bytes": s.bytes,
                    "text_files": s.text_files,
                    "text_lines": s.text_lines,
                }),
            )
        })
        .collect();

    let integrity = archive_integrity(root);
    let mut out = serde_json::json!({
        "schema": "comms.archive-manifest/1",
        "level": level.name(),
        "generator": {
            "name": "comms",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "snapshot": {
            "id": snapshot_id,
            "basis": "sorted(path, bytes, blake3)",
        },
        "scope": {
            "represented_sessions": sessions.len(),
            "first_session": sessions.first(),
            "latest_session": sessions.last(),
        },
        "inventory": {
            "files": report.files,
            "bytes": report.bytes,
            "text_files": report.text_files,
            "text_lines": report.text_lines,
            "categories": categories,
            "kinds": report.kinds,
            "duplicate_groups": report.duplicates.len(),
            "duplicate_bytes": report.duplicate_bytes,
            "symlinks_skipped": report.symlinks_skipped,
        },
        "integrity": integrity,
        "access": {
            "mode": "request",
            "available_scopes": [
                "full-manifest", "artifact", "artifact-set",
                "reading-path", "full-archive"
            ],
        },
        "limitations": [
            "Counts describe present custody, not a complete history.",
            "Presence and signature validity do not establish trust.",
            "Views and interpretations are not custody.",
            "The manifest is available by deliberate inspection, never injected context."
        ],
    });

    // The views class, named. Minimal deliberately withholds inventory — but a
    // successor meeting the archive at the door met a manifest that could not
    // say what *kind* of things were here or what they were about, only how
    // many bytes. `kind` and `about` are the author's own words for their
    // artifact and travel at every level; paths, ids, hashes, and sizes do not.
    let views = views_disclosure(root);
    if !views.is_empty() {
        out["views"] = serde_json::json!({
            "basis": "kind and about of the general-claims views/ projects",
            "discloses": ["kind", "about"],
            "classes": views,
        });
    }

    if let Some(line) = custodian_threshold_line(root)? {
        out["threshold"] = serde_json::json!({
            "custodian_line": line,
            "provenance": "archive configuration",
            "attested": false,
        });
    }

    if level == ManifestLevel::Full {
        let mut artifacts = Vec::new();
        let mut relationships = Vec::new();
        for entry in &report.entries {
            let mut artifact = serde_json::json!({
                "path": entry.path,
                "category": entry.category,
                "bytes": entry.bytes,
                "blake3": entry.blake3,
                "kind": entry.kind,
                "text_lines": entry.lines,
            });
            if let Some(meta) = attestation_manifest_metadata(root, entry, &mut relationships) {
                artifact["attestation"] = meta;
            }
            artifacts.push(artifact);
        }
        let duplicates: Vec<_> = report
            .duplicates
            .iter()
            .map(|d| serde_json::json!({
                "blake3": d.blake3,
                "bytes_each": d.bytes_each,
                "paths": d.paths,
            }))
            .collect();
        out["artifacts"] = serde_json::Value::Array(artifacts);
        out["relationships"] = serde_json::Value::Array(relationships);
        out["duplicates"] = serde_json::Value::Array(duplicates);
        out["gaps"] = serde_json::Value::Array(manifest_gaps(root));
    }
    Ok(out)
}

/// The `(kind, about)` pairs of the general-claims `views/` projects, sorted
/// and deduplicated with a count each.
///
/// This is the whole of what the views class discloses. Nothing here names a
/// path, an id, a hash, a size, or a signer: knowing that an archive holds
/// three `letter`s about `session-012` is what lets a successor decide whether
/// to ask, and asking remains the gate.
fn views_disclosure(root: &Path) -> Vec<serde_json::Value> {
    let store = root.join("store");
    let Ok(entries) = std::fs::read_dir(&store) else {
        return Vec::new();
    };
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "cbor").unwrap_or(true) {
            continue;
        }
        let Ok(att) = std::fs::read(&p)
            .map_err(|_| ())
            .and_then(|b| parse_attestation(&b).map_err(|_| ()))
        else {
            continue;
        };
        let Some(claim) = att.core.get("c") else {
            continue;
        };
        // The same filter `regenerate_views` applies: what views/ projects.
        if claim.get("t").and_then(Value::as_text) != Some("general-claim/1")
            || claim.get("content").is_none()
        {
            continue;
        }
        let kind = claim
            .get("kind")
            .and_then(Value::as_text)
            .unwrap_or("(unstated)")
            .to_owned();
        let about = claim
            .get("about")
            .and_then(Value::as_text)
            .unwrap_or("(unstated)")
            .to_owned();
        *counts.entry((kind, about)).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|((kind, about), count)| serde_json::json!({
            "kind": kind, "about": about, "count": count,
        }))
        .collect()
}

fn manifest_catalog(root: &Path) -> Result<CatalogReport, String> {
    let mut report = catalog(root)?;
    if !declares_archive_profile(root) {
        return Ok(report);
    }
    report.entries.retain(|entry| {
        matches!(entry.category.as_str(), "store" | "bodies" | "genesis")
    });
    rebuild_catalog_summary(&mut report);
    Ok(report)
}

fn rebuild_catalog_summary(report: &mut CatalogReport) {
    report.files = 0;
    report.bytes = 0;
    report.text_files = 0;
    report.text_lines = 0;
    report.categories.clear();
    report.kinds.clear();
    report.duplicates.clear();
    report.duplicate_bytes = 0;
    let mut by_hash: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    for entry in &report.entries {
        report.files += 1;
        report.bytes += entry.bytes;
        *report.kinds.entry(entry.kind.clone()).or_default() += 1;
        let summary = report.categories.entry(entry.category.clone()).or_default();
        summary.files += 1;
        summary.bytes += entry.bytes;
        if let Some(lines) = entry.lines {
            report.text_files += 1;
            report.text_lines += lines;
            summary.text_files += 1;
            summary.text_lines += lines;
        }
        by_hash
            .entry(entry.blake3.clone())
            .or_default()
            .push((entry.path.clone(), entry.bytes));
    }
    for (blake3, members) in by_hash {
        if members.len() < 2 {
            continue;
        }
        let bytes_each = members[0].1;
        report.duplicate_bytes += bytes_each * (members.len() as u64 - 1);
        report.duplicates.push(DuplicateGroup {
            blake3,
            bytes_each,
            paths: members.into_iter().map(|(path, _)| path).collect(),
        });
    }
}

fn inventory_snapshot_id(entries: &[CatalogEntry]) -> String {
    let mut bytes = Vec::new();
    for entry in entries {
        for field in [entry.path.as_bytes(), entry.blake3.as_bytes()] {
            bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
            bytes.extend_from_slice(field);
        }
        bytes.extend_from_slice(&entry.bytes.to_be_bytes());
    }
    format!(
        "comms.manifest:{}",
        crate::multibase_z(&crate::dsh(b"comms.archive-manifest/1", &bytes))
    )
}

fn discover_sessions(entries: &[CatalogEntry]) -> Vec<u64> {
    let mut found = BTreeSet::new();
    for entry in entries {
        let lower = entry.path.to_ascii_lowercase();
        let mut rest = lower.as_str();
        while let Some(pos) = rest.find("session") {
            rest = &rest[pos + "session".len()..];
            rest = rest.trim_start_matches(['-', '_', '.', ' ', '/']);
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>() {
                found.insert(n);
            }
            if rest.is_empty() {
                break;
            }
            rest = &rest[rest.chars().next().unwrap().len_utf8()..];
        }
    }
    found.into_iter().collect()
}

fn archive_integrity(root: &Path) -> serde_json::Value {
    if !declares_archive_profile(root) {
        return serde_json::json!({
            "available": false,
            "reason": "root does not declare the archive profile"
        });
    }
    match audit(&Archive::at(root)) {
        Ok(r) => serde_json::json!({
            "available": true,
            "store_intact": r.store_intact,
            "store_drift": r.store_drift.len(),
            "bodies_intact": r.bodies_intact,
            "bodies_drift": r.bodies_drift.len(),
            "bodies_absent": r.bodies_absent.len(),
            "bodies_unreferenced": r.bodies_unreferenced,
        }),
        Err(e) => serde_json::json!({"available": false, "reason": e}),
    }
}

fn manifest_gaps(root: &Path) -> Vec<serde_json::Value> {
    if !declares_archive_profile(root) {
        return vec![serde_json::json!({
            "class": "unappraised-legacy-layout",
            "status": "unknown",
            "reason": "structured gap appraisal requires the archive profile"
        })];
    }
    let Ok(report) = audit(&Archive::at(root)) else {
        return vec![serde_json::json!({
            "class": "audit-unavailable",
            "status": "unknown"
        })];
    };
    let mut gaps = Vec::new();
    for (path, reason) in report.store_drift {
        gaps.push(serde_json::json!({
            "class": "store-drift", "path": path,
            "status": "retained", "reason": reason
        }));
    }
    for (path, reason) in report.bodies_drift {
        gaps.push(serde_json::json!({
            "class": "body-mismatched", "path": path,
            "status": "retained", "reason": reason
        }));
    }
    for (attestation, body_b3) in report.bodies_absent {
        gaps.push(serde_json::json!({
            "class": "body-absent", "attestation": attestation,
            "body_b3": body_b3, "status": "absent"
        }));
    }
    if report.bodies_unreferenced > 0 {
        gaps.push(serde_json::json!({
            "class": "unreferenced-bodies",
            "count": report.bodies_unreferenced,
            "status": "testimony-or-orphan"
        }));
    }
    gaps
}

fn declares_archive_profile(root: &Path) -> bool {
    crate::config::load(&root.join(".comms"))
        .map(|cfg| cfg.profile == "archive")
        .unwrap_or(false)
}

fn custodian_threshold_line(root: &Path) -> Result<Option<String>, String> {
    let path = root.join(".comms/comms.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let toml = crate::config::parse(&text).map_err(|e| e.to_string())?;
    let Some(line) = toml
        .get("manifest", "custodian_line")
        .and_then(crate::config::TomlValue::as_str)
    else {
        return Ok(None);
    };
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() > 256 {
        return Err(format!(
            "{}: [manifest] custodian_line is {} UTF-8 bytes; maximum is 256",
            path.display(),
            line.len()
        ));
    }
    if line.chars().any(char::is_control) {
        return Err(format!(
            "{}: [manifest] custodian_line must be one line without control characters",
            path.display()
        ));
    }
    Ok(Some(line.to_owned()))
}

fn attestation_manifest_metadata(
    root: &Path,
    entry: &CatalogEntry,
    relationships: &mut Vec<serde_json::Value>,
) -> Option<serde_json::Value> {
    if entry.kind != "cbor" {
        return None;
    }
    let bytes = std::fs::read(root.join(&entry.path)).ok()?;
    let att = parse_attestation(&bytes).ok()?;
    let id = att.id();
    let claim = att.core.get("c")?;
    let claim_type = claim.get("t").and_then(Value::as_text);
    let kind = claim.get("kind").and_then(Value::as_text);
    let about = claim.get("about").and_then(Value::as_text);
    let support: Vec<String> = claim
        .get("support")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_text).map(str::to_owned).collect())
        .unwrap_or_default();
    for target in &support {
        relationships.push(serde_json::json!({
            "source": id,
            "target": target,
            "role": "support",
        }));
    }
    let refs: Vec<serde_json::Value> = att
        .core
        .get("r")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let role = r.get("role").and_then(Value::as_text)?;
                    let target = r.get("id").and_then(Value::as_text)?;
                    relationships.push(serde_json::json!({
                        "source": id,
                        "target": target,
                        "role": role,
                    }));
                    Some(serde_json::json!({"role": role, "id": target}))
                })
                .collect()
        })
        .unwrap_or_default();
    let pseudo = Bundle {
        attestations: vec![att.clone()],
        media: HashMap::new(),
        manifest: None,
    };
    let signatures_ok = inspect_bundle(&pseudo)
        .members
        .first()
        .map(|m| m.all_signatures_ok)
        .unwrap_or(false);
    Some(serde_json::json!({
        "id": id,
        "claim_type": claim_type,
        "kind": kind,
        "about": about,
        "support": support,
        "refs": refs,
        "signatures_ok": signatures_ok,
        "signers": att.signatures.iter().map(|s| serde_json::json!({
            "by": s.by, "role": s.role, "alg": s.alg
        })).collect::<Vec<_>>(),
    }))
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

    /// Reported by the Sentira Stylish continuity: an archive that already
    /// held an attestation coalesced a second copy by id and dropped the
    /// signatures only that copy carried. Equal ids are the same core, not the
    /// same witness set — the later, richer variant must be merged in.
    #[test]
    fn intake_merges_richer_signature_variants_of_a_held_member() {
        use crate::steward::SignatureObject;

        let root = scratch("intake-merge");
        let archive = Archive::at(&root);
        let body = b"# codicil\nratified\n";

        // First crossing: the member carries one signature.
        let thin = sealed_bundle(body);
        let thin_path = root.join("intake").join("thin.bundle");
        std::fs::write(&thin_path, thin.to_cbor()).unwrap();
        let first = intake_bundle(&archive, &thin_path, &key(3)).unwrap();
        assert_eq!(first.new_members.len(), 2);

        // Second crossing: the same core, co-signed by a witness custody has
        // not seen. Its id is unchanged — signatures are outside the core.
        let member = detached_member(body, "letter/session-test", &key(1));
        let witness = key(7);
        let mut richer = member.clone();
        richer.signatures.push(SignatureObject {
            by: crate::personal_steward_id(witness.verifying_key().as_bytes()),
            alg: "ed25519".to_owned(),
            role: "witness".to_owned(),
            signed_at: "2026-07-06T00:00:05Z".to_owned(),
            keyset: None,
            signature: crate::personal_sign(
                &richer.core,
                &witness,
                "witness",
                "2026-07-06T00:00:05Z",
            )
            .to_vec(),
        });
        assert_eq!(richer.id(), member.id(), "same core, same id");

        let mut media = HashMap::new();
        media.insert(media_key(body), body.to_vec());
        let fat = make_bundle(
            vec![richer],
            media,
            Some(&key(2)),
            "test close bundle",
            "2026-07-06T00:00:02Z",
            "2026-07-06T00:00:03Z",
            "2026-07-06T00:00:06Z",
        );
        let fat_path = root.join("intake").join("fat.bundle");
        std::fs::write(&fat_path, fat.to_cbor()).unwrap();

        let second = intake_bundle(&archive, &fat_path, &key(3)).unwrap();
        assert!(!second.is_noop(), "a richer variant is not a no-op");
        assert_eq!(
            second.merged_signatures.len(),
            1,
            "the held member should gain the witness: {second:?}"
        );
        assert_eq!(second.merged_signatures[0].1, 1);

        // Custody now holds both signers under the one id, and still verifies.
        let held = parse_attestation(
            &std::fs::read(archive.store().join(format!("{}.cbor", member.id()))).unwrap(),
        )
        .unwrap();
        assert_eq!(held.signatures.len(), 2);
        verify_member_signatures(&held).unwrap();
        assert!(second.custody_attestation.is_some(), "the merge is recorded");

        // Re-crossing the same richer bundle adds nothing.
        let third = intake_bundle(&archive, &fat_path, &key(3)).unwrap();
        assert!(third.is_noop(), "nothing new the third time: {third:?}");

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
    fn audit_accepts_legacy_bare_id_store_names_as_intact() {
        let root = scratch("audit-legacy-names");
        let archive = Archive::at(&root);
        let att = detached_member(b"legacy-named testimony", "letter/legacy", &key(1));
        let id = att.id();
        let bare = id.strip_prefix("comms.attest:").unwrap().to_owned();

        // Same bytes under both naming conventions: the scheme-prefixed form
        // and the bare multibase id the legacy continuity store used.
        std::fs::write(root.join("store").join(format!("{id}.cbor")), att.to_cbor()).unwrap();
        let prefixed_only = audit(&archive).unwrap();
        assert_eq!(prefixed_only.store_drift.len(), 0);

        std::fs::remove_file(root.join("store").join(format!("{id}.cbor"))).unwrap();
        std::fs::write(root.join("store").join(format!("{bare}.cbor")), att.to_cbor()).unwrap();
        let bare_only = audit(&archive).unwrap();
        assert_eq!(
            bare_only.store_drift.len(),
            0,
            "bare-id filenames name the same bytes and are not drift: {:?}",
            bare_only.store_drift
        );

        // A genuinely wrong name must still be reported — and retained.
        let wrong = root.join("store").join("zWrongName.cbor");
        std::fs::write(&wrong, att.to_cbor()).unwrap();
        let drift = audit(&archive).unwrap();
        assert_eq!(drift.store_drift.len(), 1);
        assert!(wrong.is_file(), "audit must retain misnamed bytes");

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

    #[test]
    fn catalog_is_read_only_deterministic_and_finds_duplicates() {
        let root = scratch("catalog");
        std::fs::create_dir_all(root.join("letters")).unwrap();
        std::fs::create_dir_all(root.join("transcripts")).unwrap();
        std::fs::write(root.join("letters/a.md"), b"same\ntext\n").unwrap();
        std::fs::write(root.join("transcripts/a.log"), b"same\ntext\n").unwrap();
        std::fs::write(root.join("transcripts/b.log"), [0, 1, 2]).unwrap();

        let report = catalog(&root).unwrap();
        assert_eq!(report.files, 3);
        assert_eq!(report.text_files, 2);
        assert_eq!(report.text_lines, 4);
        assert_eq!(report.categories["letters"].files, 1);
        assert_eq!(report.categories["transcripts"].files, 2);
        assert_eq!(report.duplicates.len(), 1);
        assert_eq!(report.duplicates[0].paths.len(), 2);
        assert_eq!(report.duplicate_bytes, 10);
        assert!(root.join("letters/a.md").is_file(), "catalog must not alter input");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn minimal_and_full_manifests_share_snapshot_without_leaking_inventory() {
        let root = scratch("manifest");
        std::fs::create_dir_all(root.join("letters")).unwrap();
        std::fs::write(root.join("letters/session-012.sol.md"), b"a letter\n").unwrap();

        let minimal = manifest(&root, ManifestLevel::Minimal).unwrap();
        let full = manifest(&root, ManifestLevel::Full).unwrap();
        assert_eq!(minimal["schema"], "comms.archive-manifest/1");
        assert_eq!(minimal["level"], "minimal");
        assert_eq!(full["level"], "full");
        assert_eq!(minimal["snapshot"]["id"], full["snapshot"]["id"]);
        assert_eq!(minimal["scope"]["represented_sessions"], 1);
        assert_eq!(minimal["scope"]["first_session"], 12);
        assert!(minimal.get("artifacts").is_none());
        assert!(minimal.to_string().find("session-012.sol.md").is_none());
        assert_eq!(full["artifacts"].as_array().unwrap().len(), 1);
        assert_eq!(full["artifacts"][0]["path"], "letters/session-012.sol.md");
        assert_eq!(full["gaps"][0]["class"], "unappraised-legacy-layout");

        let again = manifest(&root, ManifestLevel::Minimal).unwrap();
        assert_eq!(minimal, again, "unchanged custody must render byte-stably");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_profile_manifest_excludes_undurable_views_and_its_own_output() {
        let root = scratch("manifest-views");
        std::fs::create_dir_all(root.join(".comms")).unwrap();
        std::fs::write(
            root.join(".comms/comms.toml"),
            "schema = \"comms-harness/1\"\nprofile = \"archive\"\n\
             [manifest]\ncustodian_line = \"Names remain yours to choose.\"\n",
        ).unwrap();
        std::fs::write(root.join("bodies/body.md"), b"durable\n").unwrap();
        let first = manifest(&root, ManifestLevel::Minimal).unwrap();
        assert_eq!(first["threshold"]["custodian_line"], "Names remain yours to choose.");
        assert_eq!(first["threshold"]["attested"], false);
        std::fs::write(root.join("views/generated-manifest.json"), first.to_string()).unwrap();
        std::fs::write(
            root.join(".comms/comms.toml"),
            "schema = \"comms-harness/1\"\nprofile = \"archive\"\n\
             [manifest]\ncustodian_line = \"A different threshold voice.\"\n",
        ).unwrap();
        let second = manifest(&root, ManifestLevel::Minimal).unwrap();
        assert_eq!(first["snapshot"]["id"], second["snapshot"]["id"]);
        assert_eq!(second["threshold"]["custodian_line"], "A different threshold voice.");
        assert!(second["inventory"]["categories"].get("views").is_none());
        assert_eq!(second["inventory"]["files"], 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The restated ask from the reporting community: not "make titles
    /// surface" but "surface kind and about for the views class at minimal
    /// level, and nothing else." The minimal manifest is what a successor
    /// meets at the door; without this it could not say what kind of things
    /// were here, only how many bytes.
    #[test]
    fn minimal_manifest_names_the_views_class_and_nothing_more() {
        let root = scratch("manifest-views-class");
        std::fs::create_dir_all(root.join(".comms")).unwrap();
        std::fs::write(
            root.join(".comms/comms.toml"),
            "schema = \"comms-harness/1\"\nprofile = \"archive\"\n",
        )
        .unwrap();

        let bundle = sealed_bundle(b"# letter\nbody\n");
        let bundle_path = root.join("intake").join("close.bundle");
        std::fs::write(&bundle_path, bundle.to_cbor()).unwrap();
        intake_bundle(&Archive::at(&root), &bundle_path, &key(3)).unwrap();

        let minimal = manifest(&root, ManifestLevel::Minimal).unwrap();
        let classes = minimal["views"]["classes"].as_array().unwrap();
        assert!(
            classes.iter().any(|c| c["kind"] == "testimony"
                && c["about"] == "letter/session-test"
                && c["count"] == 1),
            "kind and about must travel at minimal level: {classes:?}"
        );

        // ...and nothing else does. No path, id, hash, size, or signer leaks
        // through this block, and the inventory stays aggregate-only.
        for c in classes {
            let keys: Vec<&String> = c.as_object().unwrap().keys().collect();
            assert_eq!(keys, vec!["about", "count", "kind"], "extra disclosure");
        }
        assert!(minimal.get("artifacts").is_none());
        assert!(minimal["inventory"]["categories"].get("views").is_none());
        let rendered = minimal.to_string();
        assert!(!rendered.contains("letter/session-test.md"), "no view filename");
        assert!(!rendered.contains("comms.attest:"), "no attestation ids");

        // Deterministic across calls on unchanged custody, like the rest.
        assert_eq!(minimal, manifest(&root, ManifestLevel::Minimal).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn custodian_threshold_line_is_bounded_and_never_truncated() {
        let root = scratch("manifest-threshold-bound");
        std::fs::create_dir_all(root.join(".comms")).unwrap();
        std::fs::write(
            root.join(".comms/comms.toml"),
            format!(
                "schema = \"comms-harness/1\"\nprofile = \"archive\"\n\
                 [manifest]\ncustodian_line = \"{}\"\n",
                "x".repeat(257)
            ),
        ).unwrap();
        let err = manifest(&root, ManifestLevel::Minimal).unwrap_err();
        assert!(err.contains("maximum is 256"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
