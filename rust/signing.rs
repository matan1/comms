//! Countersigning: the other half of a rite. The Python ceremony established
//! the pending-envelope handoff — `<name>.cbor` staged beside `<name>.needs.json`
//! naming who must still sign, signed wherever that key lives, then finalized
//! into the store. This module ports that flow so the counterparty (typically
//! a historian holding an ordinary OpenSSH ed25519 key) needs only this binary.
//!
//! As everywhere in this crate: a completed signature means the math holds and
//! the named key consented to these bytes — never that the claim is true.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};

use crate::bundle::parse_attestation;
use crate::bundle::{author_general_claim, ClaimSpec};
use crate::cbor::Value;
use crate::steward::{Attestation, SignatureObject};
use crate::{core_hash, keyfile, now_rfc3339, personal_steward_id, sig_payload};

/// One outstanding signature a pending attestation is waiting for.
#[derive(Debug, Clone, PartialEq)]
pub struct Need {
    pub by: String,
    pub role: String,
}

/// A staged attestation plus who still has to sign it.
pub struct PendingItem {
    pub stem: String,
    pub cbor_path: PathBuf,
    pub needs_path: PathBuf,
    pub attestation: Attestation,
    pub needs: Vec<Need>,
}

/// Appraisal state is local workflow, never protocol authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppraisalState {
    Unreviewed,
    Reviewing,
    Approved,
    AwaitingClarification,
    Deferred,
    Declined,
    Quarantined,
    Signed,
}

impl AppraisalState {
    pub fn parse(s: &str) -> Result<AppraisalState, String> {
        match s {
            "unreviewed" => Ok(AppraisalState::Unreviewed),
            "reviewing" => Ok(AppraisalState::Reviewing),
            "approved" => Ok(AppraisalState::Approved),
            "awaiting-clarification" => Ok(AppraisalState::AwaitingClarification),
            "deferred" => Ok(AppraisalState::Deferred),
            "declined" => Ok(AppraisalState::Declined),
            "quarantined" => Ok(AppraisalState::Quarantined),
            "signed" => Ok(AppraisalState::Signed),
            _ => Err(format!("unknown appraisal state '{s}'")),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            AppraisalState::Unreviewed => "unreviewed",
            AppraisalState::Reviewing => "reviewing",
            AppraisalState::Approved => "approved",
            AppraisalState::AwaitingClarification => "awaiting-clarification",
            AppraisalState::Deferred => "deferred",
            AppraisalState::Declined => "declined",
            AppraisalState::Quarantined => "quarantined",
            AppraisalState::Signed => "signed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingView {
    pub source: PathBuf,
    pub intended_store: PathBuf,
    pub stem: String,
    pub id: String,
    pub claim_type: Option<String>,
    pub kind: Option<String>,
    pub about: Option<String>,
    pub body_text: Option<String>,
    pub body_len: Option<u64>,
    pub signatures: usize,
    pub signatures_valid: bool,
    pub needs: Vec<Need>,
    pub appraisal: AppraisalState,
    pub clarification_id: Option<String>,
}

/// Discover every pending inbox named by the caller. Missing directories are
/// ordinary; malformed existing inboxes are reported rather than skipped.
pub fn discover_pending(dirs: &[PathBuf]) -> Result<Vec<PendingView>, String> {
    let mut views = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let store = dir.parent().unwrap_or(Path::new(".")).join("store");
        for item in read_pending(dir)? {
            let claim = item.attestation.core.get("c");
            let content = claim.and_then(|c| c.get("content"));
            let body = content.and_then(|c| c.get("body")).and_then(Value::as_bytes);
            let body_len = body.map(|b| b.len() as u64).or_else(|| {
                content.and_then(|c| c.get("body_len")).and_then(Value::as_u64)
            });
            let (appraisal, clarification_id) = read_review(dir, &item.stem)?;
            views.push(PendingView {
                source: dir.clone(),
                intended_store: store.clone(),
                stem: item.stem.clone(),
                id: item.attestation.id(),
                claim_type: claim.and_then(|c| c.get("t")).and_then(Value::as_text).map(str::to_owned),
                kind: claim.and_then(|c| c.get("kind")).and_then(Value::as_text).map(str::to_owned),
                about: claim.and_then(|c| c.get("about")).and_then(Value::as_text).map(str::to_owned),
                body_text: body.and_then(|b| std::str::from_utf8(b).ok()).map(str::to_owned),
                body_len,
                signatures: item.attestation.signatures.len(),
                signatures_valid: verify_existing_signatures(&item.attestation).is_ok(),
                needs: item.needs,
                appraisal,
                clarification_id,
            });
        }
    }
    views.sort_by(|a, b| (&a.source, &a.stem).cmp(&(&b.source, &b.stem)));
    Ok(views)
}

fn review_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{stem}.review.json"))
}

fn read_review(dir: &Path, stem: &str) -> Result<(AppraisalState, Option<String>), String> {
    let path = review_path(dir, stem);
    if !path.exists() {
        return Ok((AppraisalState::Unreviewed, None));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let state = value["state"].as_str().unwrap_or("unreviewed");
    let clarification = value["clarification_id"].as_str().map(str::to_owned);
    Ok((AppraisalState::parse(state)?, clarification))
}

pub fn set_appraisal_state(
    dir: &Path,
    stem: &str,
    state: AppraisalState,
    clarification_id: Option<&str>,
) -> Result<PathBuf, String> {
    if !dir.join(format!("{stem}.cbor")).is_file() {
        return Err(format!("no pending item '{stem}' in {}", dir.display()));
    }
    let path = review_path(dir, stem);
    let value = serde_json::json!({
        "schema": "comms.pending-review/1",
        "state": state.name(),
        "clarification_id": clarification_id,
    });
    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&value).unwrap()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub struct ClarificationOutcome {
    pub pending_id: String,
    pub clarification_id: String,
    pub stored_at: PathBuf,
    pub review_at: PathBuf,
}

/// Ask a signed question about a pending core without altering or endorsing it.
pub fn request_clarification(
    pending_dir: &Path,
    selector: &str,
    body: &[u8],
    questioner: &SigningKey,
    store_dir: &Path,
    issued_at: &str,
) -> Result<ClarificationOutcome, String> {
    let items = read_pending(pending_dir)?;
    let matches: Vec<_> = items
        .iter()
        .filter(|i| i.stem == selector || i.attestation.id() == selector)
        .collect();
    let item = match matches.as_slice() {
        [item] => *item,
        [] => return Err(format!("no pending item '{selector}' in {}", pending_dir.display())),
        _ => return Err(format!("pending selector '{selector}' is ambiguous")),
    };
    let pending_id = item.attestation.id();
    let refs = vec![("clarifies".to_owned(), pending_id.clone())];
    let spec = ClaimSpec {
        about: &pending_id,
        kind: "clarification-request",
        body,
        media_type: "text/markdown",
        support: &[],
        detach: false,
        refs: &refs,
        language: "zxx",
        community: None,
        occasion: Some("pending-appraisal"),
        issued_at,
    };
    let att = author_general_claim(&spec, questioner, "questioner", issued_at);
    let clarification_id = att.id();
    std::fs::create_dir_all(store_dir).map_err(|e| format!("{}: {e}", store_dir.display()))?;
    let stored_at = store_file(store_dir, &clarification_id);
    std::fs::write(&stored_at, att.to_cbor())
        .map_err(|e| format!("{}: {e}", stored_at.display()))?;
    let review_at = set_appraisal_state(
        pending_dir,
        &item.stem,
        AppraisalState::AwaitingClarification,
        Some(&clarification_id),
    )?;
    Ok(ClarificationOutcome { pending_id, clarification_id, stored_at, review_at })
}

/// Load a signing key from either format a counterparty plausibly holds: a
/// steward `{seed_b58, label}` JSON file, or an OpenSSH ed25519 private key
/// (A1.3 chose pure Ed25519 exactly so keys people already have can
/// participate). Encrypted OpenSSH keys prompt for their passphrase.
pub fn load_signing_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read key {}: {e}", path.display()))?;
    if text.contains("BEGIN OPENSSH PRIVATE KEY") {
        load_openssh_ed25519(path, &text)
    } else {
        keyfile::load(path)
    }
}

fn load_openssh_ed25519(path: &Path, text: &str) -> Result<SigningKey, String> {
    let key = ssh_key::PrivateKey::from_openssh(text)
        .map_err(|e| format!("{}: not a readable OpenSSH key: {e}", path.display()))?;
    let key = if key.is_encrypted() {
        let pw = rpassword::prompt_password(format!("passphrase for {}: ", path.display()))
            .map_err(|e| format!("reading passphrase: {e}"))?;
        key.decrypt(pw.as_bytes())
            .map_err(|_| format!("{}: wrong passphrase (or unsupported cipher)", path.display()))?
    } else {
        key
    };
    match key.key_data() {
        ssh_key::private::KeypairData::Ed25519(kp) => {
            Ok(SigningKey::from_bytes(&kp.private.to_bytes()))
        }
        other => Err(format!(
            "{}: is a {} key; only ed25519 participates in Attest 1.0",
            path.display(),
            other.algorithm().map(|a| a.to_string()).unwrap_or_else(|_| "non-ed25519".into())
        )),
    }
}

/// Read every staged pair in `dir`, sorted by stem. A `.cbor` without a
/// matching `.needs.json` is not a pending item and is left alone.
pub fn read_pending(dir: &Path) -> Result<Vec<PendingItem>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "cbor").unwrap_or(false))
        .collect();
    paths.sort();
    for cbor_path in paths {
        let stem = cbor_path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();
        let needs_path = dir.join(format!("{stem}.needs.json"));
        if !needs_path.exists() {
            continue;
        }
        let bytes =
            std::fs::read(&cbor_path).map_err(|e| format!("{}: {e}", cbor_path.display()))?;
        let attestation =
            parse_attestation(&bytes).map_err(|e| format!("{}: {e}", cbor_path.display()))?;
        let needs = parse_needs(&needs_path)?;
        out.push(PendingItem { stem, cbor_path, needs_path, attestation, needs });
    }
    Ok(out)
}

fn parse_needs(path: &Path) -> Result<Vec<Need>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: not JSON: {e}", path.display()))?;
    let arr = v.as_array().ok_or_else(|| format!("{}: not a JSON array", path.display()))?;
    arr.iter()
        .map(|n| {
            Ok(Need {
                by: n["by"]
                    .as_str()
                    .ok_or_else(|| format!("{}: need without 'by'", path.display()))?
                    .to_owned(),
                role: n["role"]
                    .as_str()
                    .ok_or_else(|| format!("{}: need without 'role'", path.display()))?
                    .to_owned(),
            })
        })
        .collect()
}

/// Write a pending pair. Public so rite verbs (`countersign`) can stage items
/// in the same format the ceremony established.
pub fn write_pending(
    dir: &Path,
    stem: &str,
    att: &Attestation,
    needs: &[Need],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let cbor_path = dir.join(format!("{stem}.cbor"));
    std::fs::write(&cbor_path, att.to_cbor())
        .map_err(|e| format!("{}: {e}", cbor_path.display()))?;
    let needs_json: Vec<serde_json::Value> = needs
        .iter()
        .map(|n| serde_json::json!({"by": n.by, "role": n.role}))
        .collect();
    let needs_path = dir.join(format!("{stem}.needs.json"));
    std::fs::write(&needs_path, serde_json::to_string_pretty(&needs_json).unwrap())
        .map_err(|e| format!("{}: {e}", needs_path.display()))?;
    Ok(cbor_path)
}

/// What `sign_pending` did to one item.
pub struct SignOutcome {
    pub stem: String,
    pub signed_roles: Vec<String>,
    pub skipped: Vec<Need>,
}

/// Sign every pending need in `dir` that this key can satisfy; rewrite each
/// pair in place (signatures appended, satisfied needs removed). Needs naming
/// a different steward are reported and left standing — signing is consent,
/// so nothing is signed on another key's behalf.
pub fn sign_pending(dir: &Path, sk: &SigningKey) -> Result<Vec<SignOutcome>, String> {
    sign_pending_selected(dir, sk, &[])
}

/// Sign only explicitly selected stems or ids. An empty selector list means
/// every item and backs the legacy bulk command; non-empty plans never widen.
pub fn sign_pending_selected(
    dir: &Path,
    sk: &SigningKey,
    selectors: &[String],
) -> Result<Vec<SignOutcome>, String> {
    let signer_id = personal_steward_id(sk.verifying_key().as_bytes());
    let mut outcomes = Vec::new();
    let items = selected_items(read_pending(dir)?, selectors)?;
    for mut item in items {
        let mut signed_roles = Vec::new();
        let mut remaining = Vec::new();
        for need in item.needs.clone() {
            if need.by != signer_id {
                remaining.push(need);
                continue;
            }
            let signed_at = now_rfc3339();
            let payload = sig_payload(
                &core_hash(&item.attestation.core),
                &need.by,
                "ed25519",
                &need.role,
                &signed_at,
                None,
            );
            item.attestation.signatures.push(SignatureObject {
                by: need.by.clone(),
                alg: "ed25519".to_owned(),
                role: need.role.clone(),
                signed_at,
                keyset: None,
                signature: sk.sign(&payload).to_bytes().to_vec(),
            });
            signed_roles.push(need.role);
        }
        if !signed_roles.is_empty() {
            write_pending(dir, &item.stem, &item.attestation, &remaining)?;
        }
        outcomes.push(SignOutcome { stem: item.stem, signed_roles, skipped: remaining });
    }
    Ok(outcomes)
}

/// What `finalize_pending` did to one item.
pub enum FinalizeOutcome {
    /// Stored (or merged) into the store under its content id.
    Stored { stem: String, id: String, merged: usize },
    /// Already present with nothing new to add.
    AlreadySealed { stem: String, id: String },
}

/// Move every fully-signed pending item in `dir` into `store_dir`, verifying
/// first. Ports the ceremony's finalize semantics: abort on any invalid or
/// still-needy item (a partial seal is worse than a loud stop); merge only
/// genuinely new co-signers into an already-stored id, never restamp.
pub fn finalize_pending(dir: &Path, store_dir: &Path) -> Result<Vec<FinalizeOutcome>, String> {
    finalize_pending_selected(dir, store_dir, &[])
}

/// Finalize only explicitly selected stems or ids. Every selected item must be
/// fully signed before any selected item moves; unrelated inbox items cannot
/// block or be swept into the transaction.
pub fn finalize_pending_selected(
    dir: &Path,
    store_dir: &Path,
    selectors: &[String],
) -> Result<Vec<FinalizeOutcome>, String> {
    let items = selected_items(read_pending(dir)?, selectors)?;
    // Verify everything before storing anything.
    for item in &items {
        if !item.needs.is_empty() {
            let who: Vec<&str> = item.needs.iter().map(|n| n.by.as_str()).collect();
            return Err(format!("{}: still needs {}", item.stem, who.join(", ")));
        }
        verify_all_signatures(&item.attestation).map_err(|e| format!("{}: {e}", item.stem))?;
    }
    std::fs::create_dir_all(store_dir).map_err(|e| format!("{}: {e}", store_dir.display()))?;

    let mut outcomes = Vec::new();
    for item in items {
        let id = item.attestation.id();
        let file = store_file(store_dir, &id);
        let outcome = if file.exists() {
            let existing_bytes =
                std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let mut existing = parse_attestation(&existing_bytes)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let merged = merge_new_signatures(&mut existing, &item.attestation);
            if merged > 0 {
                verify_all_signatures(&existing).map_err(|e| format!("{}: merged: {e}", item.stem))?;
                std::fs::write(&file, existing.to_cbor())
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                FinalizeOutcome::Stored { stem: item.stem.clone(), id, merged }
            } else {
                FinalizeOutcome::AlreadySealed { stem: item.stem.clone(), id }
            }
        } else {
            std::fs::write(&file, item.attestation.to_cbor())
                .map_err(|e| format!("{}: {e}", file.display()))?;
            FinalizeOutcome::Stored { stem: item.stem.clone(), id, merged: 0 }
        };
        std::fs::remove_file(&item.cbor_path)
            .map_err(|e| format!("{}: {e}", item.cbor_path.display()))?;
        std::fs::remove_file(&item.needs_path)
            .map_err(|e| format!("{}: {e}", item.needs_path.display()))?;
        let review = review_path(dir, &item.stem);
        if review.exists() {
            std::fs::remove_file(&review).map_err(|e| format!("{}: {e}", review.display()))?;
        }
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

fn selected_items(items: Vec<PendingItem>, selectors: &[String]) -> Result<Vec<PendingItem>, String> {
    if selectors.is_empty() {
        return Ok(items);
    }
    let mut selected = Vec::new();
    let mut matched = vec![false; selectors.len()];
    for item in items {
        let id = item.attestation.id();
        if let Some((idx, _)) = selectors
            .iter()
            .enumerate()
            .find(|(_, s)| **s == item.stem || **s == id)
        {
            matched[idx] = true;
            selected.push(item);
        }
    }
    let missing: Vec<&str> = selectors
        .iter()
        .zip(matched)
        .filter_map(|(s, found)| if found { None } else { Some(s.as_str()) })
        .collect();
    if !missing.is_empty() {
        return Err(format!("pending selector(s) not found: {}", missing.join(", ")));
    }
    Ok(selected)
}

/// Store filename for an attestation id: the multibase part + `.cbor`,
/// matching `store.py:Store.put`.
pub fn store_file(store_dir: &Path, id: &str) -> PathBuf {
    store_dir.join(format!("{}.cbor", id.rsplit(':').next().unwrap_or(id)))
}

/// Every signature must be a valid personal ed25519 signature and there must
/// be at least one. (Threshold `ed25519-set/1` signatures are bundle-level
/// machinery; pending items are personal acts.)
pub fn verify_existing_signatures(att: &Attestation) -> Result<(), String> {
    for sig in &att.signatures {
        if sig.alg != "ed25519" {
            return Err(format!("signature by {} has alg {}, not ed25519", sig.by, sig.alg));
        }
        let Some(pk) = crate::bundle::pubkey_from_steward_id(&sig.by) else {
            return Err(format!("malformed steward id {}", sig.by));
        };
        let Ok(raw) = <[u8; 64]>::try_from(sig.signature.as_slice()) else {
            return Err(format!("signature by {} is not 64 bytes", sig.by));
        };
        if !crate::personal_verify(&att.core, &sig.by, &sig.role, &sig.signed_at, &pk, &raw) {
            return Err(format!("signature by {} ({}) does not verify", sig.by, sig.role));
        }
    }
    Ok(())
}

fn verify_all_signatures(att: &Attestation) -> Result<(), String> {
    if att.signatures.is_empty() {
        return Err("no signatures".to_owned());
    }
    verify_existing_signatures(att)
}

/// Add to `existing` only signatures whose signer is not already present —
/// a new co-signer can join, no one gets silently restamped.
fn merge_new_signatures(existing: &mut Attestation, incoming: &Attestation) -> usize {
    let mut have: Vec<String> = existing.signatures.iter().map(|s| s.by.clone()).collect();
    let mut added = 0;
    for s in &incoming.signatures {
        if !have.contains(&s.by) {
            existing.signatures.push(s.clone());
            have.push(s.by.clone());
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{author_general_claim, ClaimSpec};

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("comms-signing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn spec<'a>(body: &'a [u8], issued_at: &'a str) -> ClaimSpec<'a> {
        ClaimSpec {
            about: "test",
            kind: "testimony",
            body,
            media_type: "text/plain",
            support: &[],
            detach: false,
            refs: &[],
            language: "zxx",
            community: None,
            occasion: None,
            issued_at,
        }
    }

    /// Unencrypted OpenSSH ed25519 fixture (generated for this test; never a
    /// live key). Signing with it must produce a signature `personal_verify`
    /// accepts under the steward id derived from its public key.
    const OPENSSH_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
QyNTUxOQAAACCnJ9dIpWkp/YIpYFN4YFlYeMmz9s05zLEPYWeISG3NuQAAAJjvFfpi7xX6\n\
YgAAAAtzc2gtZWQyNTUxOQAAACCnJ9dIpWkp/YIpYFN4YFlYeMmz9s05zLEPYWeISG3NuQ\n\
AAAEAHvnghU9X/rjv/STREz7m742cbHImcYVrGXuXMxQYv8qcn10ilaSn9gilgU3hgWVh4\n\
ybP2zTnMsQ9hZ4hIbc25AAAAEmNvbW1zLXNpZ25pbmctdGVzdAECAw==\n\
-----END OPENSSH PRIVATE KEY-----\n";

    #[test]
    fn openssh_key_loads_and_signs_verifiably() {
        let dir = tmpdir("openssh");
        let kp = dir.join("test_ed25519");
        std::fs::write(&kp, OPENSSH_KEY).unwrap();
        let sk = load_signing_key(&kp).unwrap();
        let signer = personal_steward_id(sk.verifying_key().as_bytes());

        // Stage an attestation authored by a steward key, needing the ssh key.
        let author = keyfile::mint(&dir.join("author.json"), "author").unwrap();
        let att = author_general_claim(&spec(b"hello", "2026-01-01T00:00:00Z"), &author, "author",
            "2026-01-01T00:00:00Z");
        write_pending(&dir, "1-item", &att, &[Need { by: signer.clone(), role: "custodian".into() }])
            .unwrap();

        let outcomes = sign_pending(&dir, &sk).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].signed_roles, vec!["custodian"]);
        assert!(outcomes[0].skipped.is_empty());

        // Fully signed now; finalize stores it under its id and clears pending.
        let store = dir.join("store");
        let done = finalize_pending(&dir, &store).unwrap();
        assert_eq!(done.len(), 1);
        let stored = store_file(&store, &att.id());
        assert!(stored.exists());
        assert!(read_pending(&dir).unwrap().is_empty());

        // The stored envelope carries both signatures and both verify.
        let final_att = parse_attestation(&std::fs::read(&stored).unwrap()).unwrap();
        assert_eq!(final_att.signatures.len(), 2);
        verify_all_signatures(&final_att).unwrap();
    }

    #[test]
    fn sign_skips_needs_for_other_keys() {
        let dir = tmpdir("skip");
        let author = keyfile::mint(&dir.join("author.json"), "author").unwrap();
        let other = keyfile::mint(&dir.join("other.json"), "other").unwrap();
        let other_id = personal_steward_id(other.verifying_key().as_bytes());
        let att = author_general_claim(&spec(b"x", "2026-01-01T00:00:00Z"), &author, "author",
            "2026-01-01T00:00:00Z");
        write_pending(&dir, "1-item", &att, &[Need { by: other_id.clone(), role: "party".into() }])
            .unwrap();

        let outcomes = sign_pending(&dir, &author).unwrap();
        assert!(outcomes[0].signed_roles.is_empty());
        assert_eq!(outcomes[0].skipped, vec![Need { by: other_id, role: "party".into() }]);
        // Finalize refuses while a need stands.
        assert!(finalize_pending(&dir, &dir.join("store")).is_err());
    }

    #[test]
    fn finalize_merges_new_cosigner_without_restamping() {
        let dir = tmpdir("merge");
        let store = dir.join("store");
        let author = keyfile::mint(&dir.join("author.json"), "author").unwrap();
        let co = keyfile::mint(&dir.join("co.json"), "co").unwrap();
        let co_id = personal_steward_id(co.verifying_key().as_bytes());

        let att = author_general_claim(&spec(b"m", "2026-01-01T00:00:00Z"), &author, "author",
            "2026-01-01T00:00:00Z");
        // First finalize: author-only copy reaches the store.
        write_pending(&dir, "1-item", &att, &[]).unwrap();
        finalize_pending(&dir, &store).unwrap();
        let first = std::fs::read(store_file(&store, &att.id())).unwrap();

        // Second round: same attestation gains the co-signer.
        write_pending(&dir, "1-item", &att, &[Need { by: co_id, role: "party".into() }]).unwrap();
        sign_pending(&dir, &co).unwrap();
        finalize_pending(&dir, &store).unwrap();

        let merged = parse_attestation(&std::fs::read(store_file(&store, &att.id())).unwrap()).unwrap();
        assert_eq!(merged.signatures.len(), 2);
        // The author's original signature bytes are untouched.
        let orig = parse_attestation(&first).unwrap();
        assert_eq!(merged.signatures[0].signature, orig.signatures[0].signature);
        assert_eq!(merged.signatures[0].signed_at, orig.signatures[0].signed_at);
    }

    #[test]
    fn discovery_keeps_multiple_pending_inboxes_and_destinations_distinct() {
        let root = tmpdir("discover");
        let modern = root.join(".comms/pending");
        let legacy = root.join("continuity/pending");
        let signer = keyfile::mint(&root.join("signer.json"), "signer").unwrap();
        let signer_id = personal_steward_id(signer.verifying_key().as_bytes());
        let att = author_general_claim(
            &spec(b"please review", "2026-01-01T00:00:00Z"),
            &signer,
            "author",
            "2026-01-01T00:00:00Z",
        );
        write_pending(&modern, "modern", &att, &[Need { by: signer_id.clone(), role: "party".into() }]).unwrap();
        write_pending(&legacy, "legacy", &att, &[Need { by: signer_id, role: "custodian".into() }]).unwrap();

        let views = discover_pending(&[modern.clone(), legacy.clone()]).unwrap();
        assert_eq!(views.len(), 2);
        assert!(views.iter().any(|v| v.source == modern && v.intended_store == root.join(".comms/store")));
        assert!(views.iter().any(|v| v.source == legacy && v.intended_store == root.join("continuity/store")));
        assert!(views.iter().all(|v| v.signatures_valid));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clarification_is_signed_and_marks_pending_without_endorsing_it() {
        let root = tmpdir("clarify");
        let pending = root.join(".comms/pending");
        let store = root.join(".comms/store");
        let author = keyfile::mint(&root.join("author.json"), "author").unwrap();
        let reviewer = keyfile::mint(&root.join("reviewer.json"), "reviewer").unwrap();
        let reviewer_id = personal_steward_id(reviewer.verifying_key().as_bytes());
        let item = author_general_claim(
            &spec(b"ambiguous claim", "2026-01-01T00:00:00Z"),
            &author,
            "author",
            "2026-01-01T00:00:00Z",
        );
        write_pending(&pending, "ambiguous", &item, &[Need { by: reviewer_id, role: "party".into() }]).unwrap();

        let out = request_clarification(
            &pending,
            "ambiguous",
            b"Where did this body originate?",
            &reviewer,
            &store,
            "2026-01-02T00:00:00Z",
        ).unwrap();
        assert_eq!(out.pending_id, item.id());
        assert!(out.stored_at.is_file());
        assert!(pending.join("ambiguous.cbor").is_file(), "question must not consume pending item");

        let question = parse_attestation(&std::fs::read(&out.stored_at).unwrap()).unwrap();
        verify_all_signatures(&question).unwrap();
        assert_eq!(question.signatures[0].role, "questioner");
        assert_eq!(question.core.get("c").unwrap().get("kind").and_then(Value::as_text), Some("clarification-request"));
        assert_eq!(question.core.get("c").unwrap().get("about").and_then(Value::as_text), Some(item.id().as_str()));

        let views = discover_pending(std::slice::from_ref(&pending)).unwrap();
        assert_eq!(views[0].appraisal, AppraisalState::AwaitingClarification);
        assert_eq!(views[0].clarification_id.as_deref(), Some(out.clarification_id.as_str()));
        assert_eq!(views[0].signatures, 1, "clarification must not add a signature to pending core");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn selected_sign_and_finalize_never_sweep_unrelated_items() {
        let root = tmpdir("selected");
        let pending = root.join("pending");
        let store = root.join("store");
        let author = keyfile::mint(&root.join("author.json"), "author").unwrap();
        let signer = keyfile::mint(&root.join("signer.json"), "signer").unwrap();
        let signer_id = personal_steward_id(signer.verifying_key().as_bytes());
        for stem in ["one", "two"] {
            let att = author_general_claim(
                &spec(stem.as_bytes(), "2026-01-01T00:00:00Z"),
                &author,
                "author",
                "2026-01-01T00:00:00Z",
            );
            write_pending(&pending, stem, &att, &[Need { by: signer_id.clone(), role: "party".into() }]).unwrap();
        }

        sign_pending_selected(&pending, &signer, &["one".into()]).unwrap();
        finalize_pending_selected(&pending, &store, &["one".into()]).unwrap();
        assert!(!pending.join("one.cbor").exists());
        assert!(pending.join("two.cbor").exists());
        let remaining = read_pending(&pending).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].stem, "two");
        assert_eq!(remaining[0].needs.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
