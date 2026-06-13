//! Sneakernet bundle parsing and A1.8 integrity seal verification.
//!
//! A bundle is a container, not an attestation: bare wire format makes no
//! membership guarantees. The A1.8 seal closes that: a signed general-claim
//! whose body enumerates member ids and binds them with
//! H("comms.bundle/1", canon(manifest)). Removal or substitution of members
//! breaks the seal; per-member integrity is free because each attestation id
//! is the hash of its own core.

use std::collections::{HashMap, HashSet};

use crate::{attestation_id, cbor, dsh, personal_verify, Value, CTX_BUNDLE};
use crate::steward::{Attestation, SignatureObject};

pub const BUNDLE_TYPE: &str = "comms.bundle/1";
pub const SEAL_TAG: &str = "comms.bundle.seal/1";

#[derive(Debug)]
pub enum BundleError {
    InvalidType,
    UnsupportedVersion,
    MalformedAttestation,
    MalformedSignature,
    Cbor(crate::cbor::CborError),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleError::InvalidType => write!(f, "not a comms.bundle/1 container"),
            BundleError::UnsupportedVersion => write!(f, "unsupported bundle version"),
            BundleError::MalformedAttestation => write!(f, "malformed attestation envelope"),
            BundleError::MalformedSignature => write!(f, "malformed signature object"),
            BundleError::Cbor(e) => write!(f, "CBOR error: {e}"),
        }
    }
}

impl std::error::Error for BundleError {}

pub struct Bundle {
    pub attestations: Vec<Attestation>,
    pub media: HashMap<String, Vec<u8>>,
}

/// Content-addressed key for a media blob: raw (not domain-separated) blake3,
/// multibase base58btc. Distinct from `dsh` — the spec calls this out explicitly.
pub fn media_key(data: &[u8]) -> String {
    format!("z{}", bs58::encode(blake3::hash(data).as_bytes()).into_string())
}

/// Parse one attestation from its CBOR envelope Value.
///
/// The core is the envelope minus the `s` field. Since `cbor::decode` enforces
/// canonical form and `cbor::encode` always re-produces canonical form, stripping
/// `s` and re-encoding yields the exact bytes that were originally signed.
fn attestation_from_value(v: &Value) -> Result<Attestation, BundleError> {
    let Value::Map(entries) = v else {
        return Err(BundleError::MalformedAttestation);
    };

    let sig_entry = entries
        .iter()
        .find(|(k, _)| k.as_text() == Some("s"))
        .ok_or(BundleError::MalformedAttestation)?;

    let sigs_val = match &sig_entry.1 {
        Value::Array(a) => a,
        _ => return Err(BundleError::MalformedAttestation),
    };

    let core = Value::Map(
        entries
            .iter()
            .filter(|(k, _)| k.as_text() != Some("s"))
            .cloned()
            .collect(),
    );

    let signatures = sigs_val
        .iter()
        .map(|sig| {
            Ok(SignatureObject {
                by: sig
                    .get("by")
                    .and_then(Value::as_text)
                    .ok_or(BundleError::MalformedSignature)?
                    .to_owned(),
                alg: sig
                    .get("alg")
                    .and_then(Value::as_text)
                    .ok_or(BundleError::MalformedSignature)?
                    .to_owned(),
                role: sig
                    .get("role")
                    .and_then(Value::as_text)
                    .ok_or(BundleError::MalformedSignature)?
                    .to_owned(),
                signed_at: sig
                    .get("signed_at")
                    .and_then(Value::as_text)
                    .ok_or(BundleError::MalformedSignature)?
                    .to_owned(),
                keyset: sig.get("keyset").and_then(Value::as_text).map(str::to_owned),
                signature: sig
                    .get("signature")
                    .and_then(Value::as_bytes)
                    .ok_or(BundleError::MalformedSignature)?
                    .to_vec(),
            })
        })
        .collect::<Result<Vec<_>, BundleError>>()?;

    Ok(Attestation { core, signatures })
}

/// Parse a bundle from its canonical CBOR wire bytes.
pub fn parse_bundle(data: &[u8]) -> Result<Bundle, BundleError> {
    let v = cbor::decode(data).map_err(BundleError::Cbor)?;

    if v.get("t").and_then(Value::as_text) != Some(BUNDLE_TYPE) {
        return Err(BundleError::InvalidType);
    }
    if v.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(BundleError::UnsupportedVersion);
    }

    let attestations = v
        .get("attestations")
        .and_then(Value::as_array)
        .ok_or(BundleError::MalformedAttestation)?
        .iter()
        .map(attestation_from_value)
        .collect::<Result<Vec<_>, _>>()?;

    let media = if let Some(Value::Map(entries)) = v.get("media") {
        entries
            .iter()
            .filter_map(|(k, val)| {
                let key = k.as_text()?.to_owned();
                let bytes = val.as_bytes()?.to_vec();
                Some((key, bytes))
            })
            .collect()
    } else {
        HashMap::new()
    };

    Ok(Bundle { attestations, media })
}

/// Identify A1.8 seal attestations. A seal is a `general-claim/1` with
/// `content.media_type == "application/cbor"` whose body decodes to a CBOR
/// map carrying `t == "comms.bundle.seal/1"`.
fn find_seals(bundle: &Bundle) -> Vec<(&Attestation, Value)> {
    bundle
        .attestations
        .iter()
        .filter_map(|att| {
            let claim = att.core.get("c")?;
            if claim.get("t").and_then(Value::as_text) != Some("general-claim/1") {
                return None;
            }
            let content = claim.get("content")?;
            if content.get("media_type").and_then(Value::as_text) != Some("application/cbor") {
                return None;
            }
            let body_bytes = content.get("body").and_then(Value::as_bytes)?;
            let body = cbor::decode(body_bytes).ok()?;
            if body.get("t").and_then(Value::as_text) != Some(SEAL_TAG) {
                return None;
            }
            Some((att, body))
        })
        .collect()
}

/// Extract the 32-byte public key from a personal steward id
/// `comms.steward:z<base58btc(key)>`.
fn pubkey_from_steward_id(id: &str) -> Option<[u8; 32]> {
    let encoded = id.strip_prefix("comms.steward:z")?;
    let bytes = bs58::decode(encoded).into_vec().ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

#[derive(Debug)]
pub struct SealReport {
    pub ok: bool,
    pub sealed_by: Option<String>,
    pub signature_ok: bool,
    pub hash_ok: bool,
    pub members_match: bool,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

/// Verify a bundle's A1.8 seal.
///
/// Returns a `SealReport` decomposing each check. `ok` is true iff all of:
/// - exactly one seal is present,
/// - its personal signature verifies,
/// - its `bundle_hash` matches `H("comms.bundle/1", canon(manifest))`,
/// - the sealed id set equals the set of non-seal member ids.
pub fn verify_seal(bundle: &Bundle) -> SealReport {
    let seals = find_seals(bundle);

    let mut report = SealReport {
        ok: false,
        sealed_by: None,
        signature_ok: false,
        hash_ok: false,
        members_match: false,
        missing: Vec::new(),
        extra: Vec::new(),
    };

    if seals.len() != 1 {
        return report;
    }

    let (seal_att, body) = &seals[0];
    let seal_id = attestation_id(&seal_att.core);

    // Verify the seal's personal signature
    if let Some(sig) = seal_att.signatures.first() {
        report.sealed_by = Some(sig.by.clone());
        if sig.alg == "ed25519" {
            if let (Some(pk), Ok(raw_sig)) = (
                pubkey_from_steward_id(&sig.by),
                <[u8; 64]>::try_from(sig.signature.as_slice()),
            ) {
                report.signature_ok = personal_verify(
                    &seal_att.core,
                    &sig.by,
                    &sig.role,
                    &sig.signed_at,
                    &pk,
                    &raw_sig,
                );
            }
        }
    }

    // Verify bundle_hash = H(CTX_BUNDLE, canon(manifest))
    if let Some(manifest) = body.get("manifest") {
        let expected_hash = dsh(CTX_BUNDLE, &cbor::encode(manifest));
        report.hash_ok = body
            .get("bundle_hash")
            .and_then(Value::as_bytes)
            .map(|b| b == expected_hash)
            .unwrap_or(false);

        // Compare sealed id set vs present non-seal member ids
        let sealed_ids: HashSet<String> = manifest
            .get("attestation_ids")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_text)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let present_ids: HashSet<String> = bundle
            .attestations
            .iter()
            .filter_map(|att| {
                let id = attestation_id(&att.core);
                if id == seal_id { None } else { Some(id) }
            })
            .collect();

        let mut missing: Vec<String> = sealed_ids.difference(&present_ids).cloned().collect();
        let mut extra: Vec<String> = present_ids.difference(&sealed_ids).cloned().collect();
        missing.sort();
        extra.sort();
        report.members_match = missing.is_empty() && extra.is_empty();
        report.missing = missing;
        report.extra = extra;
    }

    report.ok = report.signature_ok && report.hash_ok && report.members_match;
    report
}
