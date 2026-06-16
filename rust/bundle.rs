//! Sneakernet bundle parsing and A1.8 integrity seal verification.
//!
//! A bundle is a container, not an attestation: bare wire format makes no
//! membership guarantees. The A1.8 seal closes that: a signed general-claim
//! whose body enumerates member ids and binds them with
//! H("comms.bundle/1", canon(manifest)). Removal or substitution of members
//! breaks the seal; per-member integrity is free because each attestation id
//! is the hash of its own core.

use std::collections::{HashMap, HashSet};

use ed25519_dalek::SigningKey;

use crate::{
    attestation_id, cbor, dsh, personal_sign, personal_steward_id, personal_verify, Value,
    CTX_BUNDLE,
};
use crate::steward::{verify_community_attestation, Attestation, SignatureObject};

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
    /// Optional informational bundle manifest `{created_at, description,
    /// created_by?}`. Distinct from the A1.8 *seal* manifest (which enumerates
    /// member ids); this one is metadata only and is not covered by the seal.
    pub manifest: Option<Value>,
}

impl Bundle {
    /// Canonical CBOR of the bundle container. Mirrors
    /// `comms/bundle.py:Bundle.to_cbor`: `{v, t, attestations}` plus `media` and
    /// `manifest` only when non-empty / present.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut entries = vec![
            (Value::text("v"), Value::U64(1)),
            (Value::text("t"), Value::text(BUNDLE_TYPE)),
            (
                Value::text("attestations"),
                Value::Array(self.attestations.iter().map(Attestation::to_envelope_value).collect()),
            ),
        ];
        if !self.media.is_empty() {
            let media = self
                .media
                .iter()
                .map(|(k, v)| (Value::text(k), Value::Bytes(v.clone())))
                .collect();
            entries.push((Value::text("media"), Value::Map(media)));
        }
        if let Some(manifest) = &self.manifest {
            entries.push((Value::text("manifest"), manifest.clone()));
        }
        cbor::encode(&Value::Map(entries))
    }

    /// The non-seal members of this bundle (clones), in bundle order.
    pub fn members(&self) -> Vec<Attestation> {
        let seal_ids: HashSet<String> =
            find_seals(self).iter().map(|(a, _)| attestation_id(&a.core)).collect();
        self.attestations
            .iter()
            .filter(|a| !seal_ids.contains(&a.id()))
            .cloned()
            .collect()
    }

    /// Whether the bundle already carries at least one A1.8 seal.
    pub fn is_sealed(&self) -> bool {
        !find_seals(self).is_empty()
    }
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

/// Parse a single attestation from its canonical CBOR envelope bytes — for
/// reading loose `<id>.cbor` attestation files into a bundle.
pub fn parse_attestation(data: &[u8]) -> Result<Attestation, BundleError> {
    let v = cbor::decode(data).map_err(BundleError::Cbor)?;
    attestation_from_value(&v)
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

    let manifest = v.get("manifest").cloned();

    Ok(Bundle { attestations, media, manifest })
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

// ---- inspection (the receiver side) ----------------------------------------

/// One signature's verification result on a member attestation.
#[derive(Debug)]
pub struct SigReport {
    pub by: String,
    pub role: String,
    pub alg: String,
    pub ok: bool,
    /// Human-readable note: "ok", or why it failed / could not be resolved.
    pub detail: String,
}

/// One reference and whether its target is present in this bundle.
#[derive(Debug)]
pub struct RefReport {
    pub role: String,
    pub id: String,
    pub resolves_in_bundle: bool,
}

/// Per-member verification result.
#[derive(Debug)]
pub struct MemberReport {
    pub id: String,
    pub claim_type: String,
    pub is_seal: bool,
    pub signatures: Vec<SigReport>,
    /// True iff the member carries at least one signature and all verify.
    pub all_signatures_ok: bool,
    pub refs: Vec<RefReport>,
}

/// Whole-bundle inspection: every member verified on its own terms, media
/// content-key checks, and the A1.8 seal report.
#[derive(Debug)]
pub struct InspectReport {
    pub members: Vec<MemberReport>,
    /// (media key, whether the blob's content hash matches its key).
    pub media: Vec<(String, bool)>,
    pub seal: SealReport,
}

/// Verify every member of a bundle on its own terms — the receiver-side check
/// `verify_seal` does not do. Personal (`ed25519`) signatures are checked with
/// `personal_verify`; community (`ed25519-set/1`) signatures are resolved
/// through the keyset chain *within this bundle* and threshold-checked. Refs are
/// marked resolvable iff their target attestation is present here. A `true`
/// remains layer-2/3 (verified + resolvable), never a trust judgment.
pub fn inspect_bundle(bundle: &Bundle) -> InspectReport {
    let seal_ids: HashSet<String> =
        find_seals(bundle).iter().map(|(a, _)| attestation_id(&a.core)).collect();
    let present_ids: HashSet<String> =
        bundle.attestations.iter().map(|a| attestation_id(&a.core)).collect();

    // A store over the bundle's own members lets community signatures resolve
    // their keyset chains offline, the sneakernet norm (A1.4).
    let store: HashMap<String, Attestation> = bundle
        .attestations
        .iter()
        .map(|a| (a.id(), a.clone()))
        .collect();

    let mut members = Vec::new();
    for att in &bundle.attestations {
        let id = att.id();
        let claim_type = att
            .core
            .get("c")
            .and_then(|c| c.get("t"))
            .and_then(Value::as_text)
            .unwrap_or("?")
            .to_owned();

        let mut signatures = Vec::new();
        for sig in &att.signatures {
            let (ok, detail) = verify_member_signature(att, sig, &store);
            signatures.push(SigReport {
                by: sig.by.clone(),
                role: sig.role.clone(),
                alg: sig.alg.clone(),
                ok,
                detail,
            });
        }
        let all_signatures_ok = !signatures.is_empty() && signatures.iter().all(|s| s.ok);

        let refs = att
            .core
            .get("r")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        let rid = r.get("id").and_then(Value::as_text)?.to_owned();
                        let role = r
                            .get("role")
                            .and_then(Value::as_text)
                            .unwrap_or("?")
                            .to_owned();
                        Some(RefReport {
                            resolves_in_bundle: present_ids.contains(&rid),
                            role,
                            id: rid,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        members.push(MemberReport {
            is_seal: seal_ids.contains(&id),
            id,
            claim_type,
            signatures,
            all_signatures_ok,
            refs,
        });
    }

    let media = bundle
        .media
        .iter()
        .map(|(k, blob)| (k.clone(), media_key(blob) == *k))
        .collect();

    InspectReport { members, media, seal: verify_seal(bundle) }
}

/// Verify a single signature object against its attestation, returning
/// (ok, human-readable detail).
fn verify_member_signature(
    att: &Attestation,
    sig: &SignatureObject,
    store: &HashMap<String, Attestation>,
) -> (bool, String) {
    match sig.alg.as_str() {
        "ed25519" => {
            let Some(pk) = pubkey_from_steward_id(&sig.by) else {
                return (false, "personal: malformed steward id".to_owned());
            };
            let Ok(raw) = <[u8; 64]>::try_from(sig.signature.as_slice()) else {
                return (false, "personal: signature not 64 bytes".to_owned());
            };
            if personal_verify(&att.core, &sig.by, &sig.role, &sig.signed_at, &pk, &raw) {
                (true, "ed25519 ok".to_owned())
            } else {
                (false, "ed25519 invalid".to_owned())
            }
        }
        "ed25519-set/1" => match verify_community_attestation(att, &sig.by, store) {
            Ok(()) => (true, "ed25519-set/1 ok (threshold met)".to_owned()),
            Err(e) => (false, format!("ed25519-set/1: {e}")),
        },
        other => (false, format!("unknown alg {other}")),
    }
}

// ---- creation (the courier side) -------------------------------------------

/// Build the A1.8 seal manifest `{created_at, created_by, description,
/// attestation_ids}` over `member_ids` (sorted). Port of
/// `comms/bundle.py:_seal_manifest`.
fn seal_manifest(member_ids: &[String], created_by: &str, description: &str, created_at: &str) -> Value {
    let mut ids: Vec<&String> = member_ids.iter().collect();
    ids.sort();
    Value::Map(vec![
        (Value::text("created_at"), Value::text(created_at)),
        (Value::text("created_by"), Value::text(created_by)),
        (Value::text("description"), Value::text(description)),
        (
            Value::text("attestation_ids"),
            Value::Array(ids.into_iter().map(|s| Value::text(s)).collect()),
        ),
    ])
}

/// Build the A1.8 integrity seal over `members`, signed by `sk`. Faithful port
/// of `comms/bundle.py:seal`: a `general-claim/1` whose CBOR body carries the
/// member-id manifest and `bundle_hash = H(CTX_BUNDLE, canon(manifest))`, signed
/// as the author. Timestamps are explicit so the bytes are reproducible (Python
/// stamps `now()` by default; byte-parity requires pinning them).
pub fn build_seal(
    members: &[Attestation],
    sk: &SigningKey,
    description: &str,
    created_at: &str,
    issued_at: &str,
    signed_at: &str,
) -> Attestation {
    let by = personal_steward_id(sk.verifying_key().as_bytes());
    let member_ids: Vec<String> = members.iter().map(Attestation::id).collect();
    let manifest = seal_manifest(&member_ids, &by, description, created_at);
    let bundle_hash = dsh(CTX_BUNDLE, &cbor::encode(&manifest));
    let body = cbor::encode(&Value::Map(vec![
        (Value::text("t"), Value::text(SEAL_TAG)),
        (Value::text("manifest"), manifest),
        (Value::text("bundle_hash"), Value::Bytes(bundle_hash.to_vec())),
    ]));

    // general-claim/1 core wrapping the CBOR seal body (claims.general_claim).
    let claim = Value::Map(vec![
        (Value::text("t"), Value::text("general-claim/1")),
        (Value::text("about"), Value::text("comms.bundle")),
        (Value::text("kind"), Value::text("synthesis")),
        (
            Value::text("content"),
            Value::Map(vec![
                (Value::text("media_type"), Value::text("application/cbor")),
                (Value::text("body"), Value::Bytes(body)),
            ]),
        ),
        (Value::text("support"), Value::Array(Vec::new())),
    ]);
    // frame: Attestation.build defaults language to "zxx"; seal adds the occasion.
    let frame = Value::Map(vec![
        (Value::text("issued_at"), Value::text(issued_at)),
        (Value::text("language"), Value::text("zxx")),
        (Value::text("occasion"), Value::text("bundle seal (A1.8)")),
    ]);
    let core = Value::Map(vec![
        (Value::text("v"), Value::U64(1)),
        (Value::text("t"), Value::text("comms.attestation/1")),
        (Value::text("c"), claim),
        (Value::text("f"), frame),
        (Value::text("r"), Value::Array(Vec::new())),
    ]);

    let signature = personal_sign(&core, sk, "author", signed_at).to_vec();
    Attestation {
        core,
        signatures: vec![SignatureObject {
            by,
            alg: "ed25519".to_owned(),
            role: "author".to_owned(),
            signed_at: signed_at.to_owned(),
            keyset: None,
            signature,
        }],
    }
}

/// What to assert in a `general-claim/1` attestation. Mirrors the parameters of
/// the Python reference `comms/claims.py:general_claim` plus the frame fields
/// `comms/attest.py:Attestation.build` adds, so an attestation authored here is
/// byte-identical to the Python one given the same inputs (canonical CBOR sorts
/// map keys, so field order does not matter).
pub struct ClaimSpec<'a> {
    pub about: &'a str,
    pub kind: &'a str,
    pub body: &'a [u8],
    /// Per A1.6 the body travels as bytes regardless of media type.
    pub media_type: &'a str,
    /// Claim-level supporting attestation ids (the `support` list).
    pub support: &'a [String],
    pub language: &'a str,
    pub community: Option<&'a str>,
    pub occasion: Option<&'a str>,
    pub issued_at: &'a str,
}

/// Author and personally sign a `general-claim/1` attestation — the creation
/// path the harness rituals (letters, transcripts, memories, …) need. It is the
/// general-purpose sibling of `build_seal`, which authors the one specific
/// general-claim that is a bundle seal. The result is a layer-1 artifact: a
/// well-formed, signed claim. Whether anyone should *believe* it is a trust
/// judgment that lives nowhere in this crate.
pub fn author_general_claim(
    spec: &ClaimSpec,
    sk: &SigningKey,
    role: &str,
    signed_at: &str,
) -> Attestation {
    let claim = Value::Map(vec![
        (Value::text("t"), Value::text("general-claim/1")),
        (Value::text("about"), Value::text(spec.about)),
        (Value::text("kind"), Value::text(spec.kind)),
        (
            Value::text("content"),
            Value::Map(vec![
                (Value::text("media_type"), Value::text(spec.media_type)),
                (Value::text("body"), Value::Bytes(spec.body.to_vec())),
            ]),
        ),
        (
            Value::text("support"),
            Value::Array(spec.support.iter().map(|s| Value::text(s)).collect()),
        ),
    ]);

    let mut frame = vec![
        (Value::text("issued_at"), Value::text(spec.issued_at)),
        (Value::text("language"), Value::text(spec.language)),
    ];
    if let Some(c) = spec.community {
        frame.push((Value::text("community"), Value::text(c)));
    }
    if let Some(o) = spec.occasion {
        frame.push((Value::text("occasion"), Value::text(o)));
    }

    let core = Value::Map(vec![
        (Value::text("v"), Value::U64(1)),
        (Value::text("t"), Value::text("comms.attestation/1")),
        (Value::text("c"), claim),
        (Value::text("f"), Value::Map(frame)),
        (Value::text("r"), Value::Array(Vec::new())),
    ]);

    let by = personal_steward_id(sk.verifying_key().as_bytes());
    let signature = personal_sign(&core, sk, role, signed_at).to_vec();
    Attestation {
        core,
        signatures: vec![SignatureObject {
            by,
            alg: "ed25519".to_owned(),
            role: role.to_owned(),
            signed_at: signed_at.to_owned(),
            keyset: None,
            signature,
        }],
    }
}

/// Assemble a bundle from `members` (+ optional `media`), optionally sealing it
/// with `sealer`. Port of `comms/bundle.py:make`: when a sealer is given an
/// A1.8 seal is appended, and an informational bundle manifest is attached when
/// there is a description or a known creator.
pub fn make_bundle(
    members: Vec<Attestation>,
    media: HashMap<String, Vec<u8>>,
    sealer: Option<&SigningKey>,
    description: &str,
    created_at: &str,
    issued_at: &str,
    signed_at: &str,
) -> Bundle {
    let mut attestations = members.clone();
    if let Some(sk) = sealer {
        attestations.push(build_seal(&members, sk, description, created_at, issued_at, signed_at));
    }

    let created_by = sealer.map(|sk| personal_steward_id(sk.verifying_key().as_bytes()));
    let manifest = if !description.is_empty() || created_by.is_some() {
        let mut entries = vec![
            (Value::text("created_at"), Value::text(created_at)),
            (Value::text("description"), Value::text(description)),
        ];
        if let Some(by) = &created_by {
            entries.push((Value::text("created_by"), Value::text(by)));
        }
        Some(Value::Map(entries))
    } else {
        None
    };

    Bundle { attestations, media, manifest }
}
