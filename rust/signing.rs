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
    let signer_id = personal_steward_id(sk.verifying_key().as_bytes());
    let mut outcomes = Vec::new();
    for mut item in read_pending(dir)? {
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
    let items = read_pending(dir)?;
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
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Store filename for an attestation id: the multibase part + `.cbor`,
/// matching `store.py:Store.put`.
pub fn store_file(store_dir: &Path, id: &str) -> PathBuf {
    store_dir.join(format!("{}.cbor", id.rsplit(':').next().unwrap_or(id)))
}

/// Every signature must be a valid personal ed25519 signature and there must
/// be at least one. (Threshold `ed25519-set/1` signatures are bundle-level
/// machinery; pending items are personal acts.)
fn verify_all_signatures(att: &Attestation) -> Result<(), String> {
    if att.signatures.is_empty() {
        return Err("no signatures".to_owned());
    }
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
}
