//! comms-core: reference core for Comms Attest 1.0 (+Amendment A1) and
//! Steward 1.0.
//!
//! Layering mirrors the specs. This crate implements the protocol substrate
//! only — well-formedness, verification, chain resolution. It deliberately
//! contains no notion of trust: a `true` from any verify function here means
//! "the math holds," never "believe this."

pub mod archive;
pub mod bundle;
pub mod cbor;
pub mod config;
pub mod init;
pub mod keyfile;
pub mod rites;
pub mod signing;
pub mod sshagent;
pub mod steward;
pub mod trial_log;
pub mod vouch;

/// RFC 3339 UTC, second precision, `Z` suffix (the A1.6 canonical timestamp).
/// Implemented without a date crate (civil_from_days, Howard Hinnant) so the
/// binary stays dependency-light.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_from_unix(secs)
}

/// RFC 3339 UTC from a unix timestamp (see [`now_rfc3339`]).
pub fn rfc3339_from_unix(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

pub use cbor::{CborError, Value};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Hash context for attestation cores (identifier derivation and signing).
pub const CTX_CORE: &[u8] = b"comms.attest.core/1";
/// Hash context for keyset descriptors (community identity derivation).
pub const CTX_KEYSET: &[u8] = b"comms.keyset/1";
/// Hash context for bundle manifests.
pub const CTX_BUNDLE: &[u8] = b"comms.bundle/1";

/// Domain-separated blake3 (Amendment A1.1):
/// `H(ctx, D) = blake3(uint8(len(ctx)) || ctx || D)`.
pub fn dsh(ctx: &[u8], data: &[u8]) -> [u8; 32] {
    assert!(ctx.len() < 256, "context string must be < 256 bytes");
    let mut h = blake3::Hasher::new();
    h.update(&[ctx.len() as u8]);
    h.update(ctx);
    h.update(data);
    *h.finalize().as_bytes()
}

/// Multibase base58btc with the `z` prefix.
pub fn multibase_z(data: &[u8]) -> String {
    format!("z{}", bs58::encode(data).into_string())
}

/// Single-key steward identifier: the bare Ed25519 public key, multibase encoded.
pub fn personal_steward_id(public_key: &[u8; 32]) -> String {
    format!("comms.steward:{}", multibase_z(public_key))
}

/// Domain-separated hash of an attestation core (the document sans `s`).
pub fn core_hash(core: &Value) -> [u8; 32] {
    dsh(CTX_CORE, &cbor::encode(core))
}

/// `comms.attest:` identifier for a core.
pub fn attestation_id(core: &Value) -> String {
    format!("comms.attest:{}", multibase_z(&core_hash(core)))
}

/// Build the canonical signature payload (Amendment A1.3, extended by
/// Steward 1.0 §3.2 with the optional `keyset` field for `ed25519-set/1`).
/// Signatures are plain Ed25519 over these raw bytes — no prehash.
pub fn sig_payload(
    core_hash: &[u8; 32],
    by: &str,
    alg: &str,
    role: &str,
    signed_at: &str,
    keyset: Option<&str>,
) -> Vec<u8> {
    let mut entries = vec![
        (Value::text("t"), Value::text("comms.sig/1")),
        (Value::text("core"), Value::Bytes(core_hash.to_vec())),
        (Value::text("by"), Value::text(by)),
        (Value::text("alg"), Value::text(alg)),
        (Value::text("role"), Value::text(role)),
        (Value::text("signed_at"), Value::text(signed_at)),
    ];
    if let Some(k) = keyset {
        entries.push((Value::text("keyset"), Value::text(k)));
    }
    cbor::encode(&Value::Map(entries))
}

/// Anything that can sign for a personal steward identity: a local seed, or a
/// key held by an agent that signs on the session's behalf so the seed never
/// touches disk (the session-10 custody fix).
pub trait StewardSigner {
    fn public_bytes(&self) -> [u8; 32];
    fn try_sign(&self, payload: &[u8]) -> Result<[u8; 64], String>;
    fn steward_id(&self) -> String {
        personal_steward_id(&self.public_bytes())
    }
}

impl StewardSigner for SigningKey {
    fn public_bytes(&self) -> [u8; 32] {
        *self.verifying_key().as_bytes()
    }
    fn try_sign(&self, payload: &[u8]) -> Result<[u8; 64], String> {
        Ok(self.sign(payload).to_bytes())
    }
}

/// Sign an attestation core with a personal (single-key) steward identity.
pub fn personal_sign(
    core: &Value,
    sk: &SigningKey,
    role: &str,
    signed_at: &str,
) -> [u8; 64] {
    let by = personal_steward_id(sk.verifying_key().as_bytes());
    let payload = sig_payload(&core_hash(core), &by, "ed25519", role, signed_at, None);
    sk.sign(&payload).to_bytes()
}

/// `personal_sign` for any signer, local or agent-held. Fallible because the
/// agent may be gone — which is exactly the state a shredded session is in.
pub fn personal_sign_with(
    core: &Value,
    signer: &dyn StewardSigner,
    role: &str,
    signed_at: &str,
) -> Result<[u8; 64], String> {
    let by = signer.steward_id();
    let payload = sig_payload(&core_hash(core), &by, "ed25519", role, signed_at, None);
    signer.try_sign(&payload)
}

/// Verify a personal signature object's fields against a core.
/// All metadata (`by`, `role`, `signed_at`) is inside the signed payload, so
/// altering any of it after signing fails here (the A1.3 role-swap fix).
pub fn personal_verify(
    core: &Value,
    by: &str,
    role: &str,
    signed_at: &str,
    public_key: &[u8; 32],
    signature: &[u8; 64],
) -> bool {
    if by != personal_steward_id(public_key) {
        return false;
    }
    let payload = sig_payload(&core_hash(core), by, "ed25519", role, signed_at, None);
    let Ok(vk) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    vk.verify(&payload, &Signature::from_bytes(signature)).is_ok()
}
