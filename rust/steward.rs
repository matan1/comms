//! Steward 1.0: keyset descriptors, genesis-anchored community identity,
//! `ed25519-set/1` threshold-by-counting signatures, rotation chains, and
//! their verification. Succession claims need no code of their own — they are
//! ordinary attestations whose *authority* is a trust-layer judgment; this
//! module only makes their community signatures and witness signatures
//! verifiable like any others.

use std::collections::HashMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use crate::{cbor, core_hash, dsh, multibase_z, sig_payload, Value, CTX_KEYSET};

/// A flat n-of-m keyset: the minimal answer to "what counts as a valid
/// signature by this steward." Names and key↔person bindings deliberately
/// live in ceremonies, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    /// Member Ed25519 public keys, sorted bytewise, unique.
    pub members: Vec<[u8; 32]>,
    pub threshold: u64,
}

impl Descriptor {
    pub fn new(mut members: Vec<[u8; 32]>, threshold: u64) -> Result<Descriptor, StewardError> {
        members.sort();
        members.dedup();
        if threshold == 0 || threshold > members.len() as u64 {
            return Err(StewardError::BadDescriptor);
        }
        Ok(Descriptor { members, threshold })
    }

    /// Canonical descriptor object: `{v: 1, members: [{key}], threshold}`.
    pub fn to_value(&self) -> Value {
        let members = self
            .members
            .iter()
            .map(|k| Value::Map(vec![(Value::text("key"), Value::Bytes(k.to_vec()))]))
            .collect();
        Value::Map(vec![
            (Value::text("v"), Value::U64(1)),
            (Value::text("members"), Value::Array(members)),
            (Value::text("threshold"), Value::U64(self.threshold)),
        ])
    }

    pub fn from_value(v: &Value) -> Result<Descriptor, StewardError> {
        let members = v
            .get("members")
            .and_then(Value::as_array)
            .ok_or(StewardError::BadDescriptor)?
            .iter()
            .map(|m| {
                m.get("key")
                    .and_then(Value::as_bytes)
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                    .ok_or(StewardError::BadDescriptor)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let threshold = v
            .get("threshold")
            .and_then(Value::as_u64)
            .ok_or(StewardError::BadDescriptor)?;
        Descriptor::new(members, threshold)
    }

    pub fn contains(&self, key: &[u8; 32]) -> bool {
        self.members.binary_search(key).is_ok()
    }
}

/// Genesis-anchored community identity (Steward 1.0 §2): the hash of the
/// **genesis** descriptor, stable across all later rotations.
pub fn community_id(genesis: &Descriptor) -> String {
    let h = dsh(CTX_KEYSET, &cbor::encode(&genesis.to_value()));
    format!("comms.steward:{}", multibase_z(&h))
}

/// A signature object as it appears in an attestation's `s` array.
#[derive(Debug, Clone)]
pub struct SignatureObject {
    pub by: String,
    pub alg: String,
    pub role: String,
    pub signed_at: String,
    /// Present iff `alg == "ed25519-set/1"`: the keyset/1 attestation ID this
    /// signature claims validity under. Inside the signed payload.
    pub keyset: Option<String>,
    /// For `ed25519`: 64 signature bytes. For `ed25519-set/1`: canonical CBOR
    /// array of inner `{k, s}` entries.
    pub signature: Vec<u8>,
}

/// An attestation: core document plus detached signatures.
#[derive(Debug, Clone)]
pub struct Attestation {
    pub core: Value,
    pub signatures: Vec<SignatureObject>,
}

impl Attestation {
    pub fn id(&self) -> String {
        crate::attestation_id(&self.core)
    }

    /// Reconstruct the signed envelope `Value`: the core map with the `s`
    /// signature array re-inserted. Inverse of `bundle::attestation_from_value`.
    /// `cbor::encode` canonicalizes key order, so this yields the same bytes the
    /// envelope was parsed from (and the same bytes Python's `to_envelope`
    /// produces for an equivalent attestation).
    pub fn to_envelope_value(&self) -> Value {
        let mut entries = match &self.core {
            Value::Map(core_entries) => core_entries.clone(),
            other => return other.clone(),
        };
        let sigs = self.signatures.iter().map(sig_to_value).collect();
        entries.push((Value::text("s"), Value::Array(sigs)));
        Value::Map(entries)
    }

    /// Canonical CBOR bytes of the full envelope (core + signatures).
    pub fn to_cbor(&self) -> Vec<u8> {
        cbor::encode(&self.to_envelope_value())
    }
}

/// Serialize a signature object to its envelope `Value`. Mirrors the parse in
/// `bundle::attestation_from_value`: `{by, alg, role, signed_at, signature}`
/// plus `keyset` iff present (set signatures).
fn sig_to_value(sig: &SignatureObject) -> Value {
    let mut entries = vec![
        (Value::text("by"), Value::text(&sig.by)),
        (Value::text("alg"), Value::text(&sig.alg)),
        (Value::text("role"), Value::text(&sig.role)),
        (Value::text("signed_at"), Value::text(&sig.signed_at)),
        (Value::text("signature"), Value::Bytes(sig.signature.clone())),
    ];
    if let Some(keyset) = &sig.keyset {
        entries.push((Value::text("keyset"), Value::text(keyset)));
    }
    Value::Map(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StewardError {
    BadDescriptor,
    UnknownKeysetAttestation(String),
    /// Genesis descriptor does not hash to the community identifier.
    GenesisIdentityMismatch,
    /// Genesis must carry a community signature referencing itself.
    GenesisNotSelfSigned,
    /// A chain link must carry exactly one `supersedes` ref.
    MalformedLink,
    /// A rotation's community signature must reference its predecessor.
    RotationNotPredecessorAuthorized,
    /// Threshold not met (or tampering detected) at some link or endpoint.
    SetSignatureInvalid,
    /// No `ed25519-set/1` signature by the named community on the attestation.
    NoCommunitySignature,
}

impl std::fmt::Display for StewardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StewardError::BadDescriptor => write!(f, "malformed keyset descriptor"),
            StewardError::UnknownKeysetAttestation(id) => {
                write!(f, "keyset attestation not resolvable in this context: {id}")
            }
            StewardError::GenesisIdentityMismatch => {
                write!(f, "genesis descriptor does not hash to the community id")
            }
            StewardError::GenesisNotSelfSigned => {
                write!(f, "genesis is not threshold-signed under its own descriptor")
            }
            StewardError::MalformedLink => {
                write!(f, "chain link must carry exactly one supersedes ref")
            }
            StewardError::RotationNotPredecessorAuthorized => {
                write!(f, "rotation is not authorized by its predecessor keyset")
            }
            StewardError::SetSignatureInvalid => {
                write!(f, "set signature does not meet threshold or shows tampering")
            }
            StewardError::NoCommunitySignature => {
                write!(f, "no community set-signature found on attestation")
            }
        }
    }
}

impl std::error::Error for StewardError {}

/// Produce a community `ed25519-set/1` signature: every signer signs the same
/// payload; the signature bytes are a canonical CBOR array of `{k, s}` sorted
/// by key. Threshold by counting — no aggregation, no ceremony, signers can
/// be collected asynchronously.
pub fn community_sign(
    core: &Value,
    by: &str,
    role: &str,
    signed_at: &str,
    keyset_attest_id: &str,
    signers: &[SigningKey],
) -> SignatureObject {
    let payload = sig_payload(
        &core_hash(core),
        by,
        "ed25519-set/1",
        role,
        signed_at,
        Some(keyset_attest_id),
    );
    let mut inner: Vec<([u8; 32], [u8; 64])> = signers
        .iter()
        .map(|sk| (*sk.verifying_key().as_bytes(), sk.sign(&payload).to_bytes()))
        .collect();
    inner.sort_by(|a, b| a.0.cmp(&b.0));
    let entries = inner
        .iter()
        .map(|(k, s)| {
            Value::Map(vec![
                (Value::text("k"), Value::Bytes(k.to_vec())),
                (Value::text("s"), Value::Bytes(s.to_vec())),
            ])
        })
        .collect();
    SignatureObject {
        by: by.to_owned(),
        alg: "ed25519-set/1".to_owned(),
        role: role.to_owned(),
        signed_at: signed_at.to_owned(),
        keyset: Some(keyset_attest_id.to_owned()),
        signature: cbor::encode(&Value::Array(entries)),
    }
}

/// Verify one `ed25519-set/1` signature object against a specific descriptor
/// (Steward 1.0 §3.2). Tolerant counting: inner signatures by keys absent
/// from the descriptor are ignored — a hostile relay can pad but not poison.
/// A *forged* signature from a listed key is fatal (tampering, not noise),
/// as are duplicate keys. Valid iff ≥ threshold distinct listed keys verify.
pub fn verify_set_signature(core: &Value, sig: &SignatureObject, desc: &Descriptor) -> bool {
    let Some(keyset) = sig.keyset.as_deref() else {
        return false;
    };
    let payload = sig_payload(
        &core_hash(core),
        &sig.by,
        "ed25519-set/1",
        &sig.role,
        &sig.signed_at,
        Some(keyset),
    );
    let Ok(Value::Array(inner)) = cbor::decode(&sig.signature) else {
        return false;
    };
    let mut seen: Vec<[u8; 32]> = Vec::new();
    let mut valid: u64 = 0;
    for entry in &inner {
        let (Some(k), Some(s)) = (
            entry.get("k").and_then(Value::as_bytes),
            entry.get("s").and_then(Value::as_bytes),
        ) else {
            return false;
        };
        let (Ok(k), Ok(s)) = (<[u8; 32]>::try_from(k), <[u8; 64]>::try_from(s)) else {
            return false;
        };
        if seen.contains(&k) {
            return false; // duplicate keys are fatal
        }
        seen.push(k);
        if !desc.contains(&k) {
            continue; // padding by a relay: ignored, not fatal
        }
        let Ok(vk) = VerifyingKey::from_bytes(&k) else {
            return false;
        };
        if vk.verify(&payload, &Signature::from_bytes(&s)).is_err() {
            return false; // forged signature from a listed key: fatal
        }
        valid += 1;
    }
    valid >= desc.threshold
}

fn supersedes_refs(core: &Value) -> Vec<String> {
    core.get("r")
        .and_then(Value::as_array)
        .map(|refs| {
            refs.iter()
                .filter(|r| r.get("role").and_then(Value::as_text) == Some("supersedes"))
                .filter_map(|r| r.get("id").and_then(Value::as_text).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn community_sig<'a>(att: &'a Attestation, community: &str) -> Option<&'a SignatureObject> {
    att.signatures
        .iter()
        .find(|s| s.alg == "ed25519-set/1" && s.by == community)
}

/// Walk a `keyset/1` attestation back to genesis (Steward 1.0 §4), returning
/// the descriptor that link establishes. Wholly offline: needs only `store`.
///
/// Genesis: no `supersedes` ref; descriptor must hash to `community`
/// (self-certifying) and the link must be threshold-signed under its own
/// descriptor, with the signature's `keyset` field referencing itself
/// (proves the founders possess the listed keys).
///
/// Rotation: exactly one `supersedes` ref; the link must carry a community
/// signature whose `keyset` names the predecessor and which meets the
/// **predecessor's** threshold. The keys as they were authorize the keys as
/// they will be.
pub fn verify_chain(
    community: &str,
    keyset_attest_id: &str,
    store: &HashMap<String, Attestation>,
) -> Result<Descriptor, StewardError> {
    let att = store
        .get(keyset_attest_id)
        .ok_or_else(|| StewardError::UnknownKeysetAttestation(keyset_attest_id.to_owned()))?;
    let desc_value = att
        .core
        .get("c")
        .and_then(|c| c.get("descriptor"))
        .ok_or(StewardError::BadDescriptor)?;
    let desc = Descriptor::from_value(desc_value)?;
    let prev = supersedes_refs(&att.core);

    if prev.is_empty() {
        if community_id(&desc) != community {
            return Err(StewardError::GenesisIdentityMismatch);
        }
        let sig = community_sig(att, community).ok_or(StewardError::GenesisNotSelfSigned)?;
        if sig.keyset.as_deref() != Some(keyset_attest_id) {
            return Err(StewardError::GenesisNotSelfSigned);
        }
        if !verify_set_signature(&att.core, sig, &desc) {
            return Err(StewardError::GenesisNotSelfSigned);
        }
        return Ok(desc);
    }

    if prev.len() != 1 {
        return Err(StewardError::MalformedLink);
    }
    let prev_desc = verify_chain(community, &prev[0], store)?;
    let sig = community_sig(att, community).ok_or(StewardError::NoCommunitySignature)?;
    if sig.keyset.as_deref() != Some(prev[0].as_str()) {
        return Err(StewardError::RotationNotPredecessorAuthorized);
    }
    if !verify_set_signature(&att.core, sig, &prev_desc) {
        return Err(StewardError::SetSignatureInvalid);
    }
    Ok(desc)
}

/// Full offline verification of a community-signed attestation: resolve the
/// signature's claimed keyset through the chain, then verify the signature
/// against that descriptor. A `true` here is layer-2/3 only (verified and
/// resolvable, per A1.4): whether the chain represents the community you
/// *mean* remains a trust judgment, outside this crate by design.
pub fn verify_community_attestation(
    att: &Attestation,
    community: &str,
    store: &HashMap<String, Attestation>,
) -> Result<(), StewardError> {
    let sig = community_sig(att, community).ok_or(StewardError::NoCommunitySignature)?;
    let keyset = sig
        .keyset
        .clone()
        .ok_or(StewardError::SetSignatureInvalid)?;
    let desc = verify_chain(community, &keyset, store)?;
    if verify_set_signature(&att.core, sig, &desc) {
        Ok(())
    } else {
        Err(StewardError::SetSignatureInvalid)
    }
}
