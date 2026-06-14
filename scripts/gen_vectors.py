"""Generate Attest 1.0 golden test vectors under the amended signature scheme.

Implements:
  - RFC 8949 deterministic CBOR (via cbor2 canonical=True; all map keys here are
    short text strings, where RFC 7049 canonical and RFC 8949 bytewise ordering
    coincide -- asserted below)
  - Domain-separated blake3: H(ctx, data) = blake3(len(ctx) as u8 || ctx || data)
  - Authenticated signature payload: Ed25519 over canonical CBOR of
    {"t": "comms.sig/1", "core": <core hash bytes>, "by", "alg", "role", "signed_at"}
"""
import json
import sys
from pathlib import Path

import cbor2
import blake3
from nacl.signing import SigningKey

OUT_DIR = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent / "data"

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def b58encode(b: bytes) -> str:
    n = int.from_bytes(b, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = B58[r] + out
    pad = 0
    for byte in b:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + out

def multibase_z(b: bytes) -> str:
    return "z" + b58encode(b)

CTX_CORE = b"comms.attest.core/1"

def dsh(ctx: bytes, data: bytes) -> bytes:
    """Domain-separated hash: blake3(uint8(len(ctx)) || ctx || data)."""
    assert len(ctx) < 256
    return blake3.blake3(bytes([len(ctx)]) + ctx + data).digest()

def canon(obj) -> bytes:
    return cbor2.dumps(obj, canonical=True)

# --- sanity: RFC 8949 bytewise key order for our keys -------------------------
# For text-string keys < 24 bytes the CBOR head byte is 0x60+len, so bytewise
# order of encodings == (length, lexicographic) == RFC 7049 canonical order.
enc = canon({"c": 1, "v": 2, "t": 3, "r": 4, "f": 5})
keys_in_order = list(cbor2.loads(enc).keys())
assert keys_in_order == sorted(keys_in_order, key=lambda k: canon(k)), keys_in_order

# --- test identities -----------------------------------------------------------
def steward(seed_byte: int):
    sk = SigningKey(bytes([seed_byte]) * 32)
    pub = sk.verify_key.encode()
    return sk, pub, "comms.steward:" + multibase_z(pub)

sk_a, pub_a, id_a = steward(0x01)
sk_w, pub_w, id_w = steward(0x02)

# --- vector 1: minimal general-claim, one author signature ---------------------
core1 = {
    "v": 1,
    "t": "comms.attestation/1",
    "c": {
        "t": "general-claim/1",
        "about": id_w,
        "kind": "observation",
        "content": {"media_type": "text/plain;charset=utf-8",
                    "body": "The northern field was sown on the new moon.".encode()},
        "support": [],
    },
    "f": {"issued_at": "2026-06-11T00:00:00Z", "language": "en"},
    "r": [],
}
core1_cbor = canon(core1)
core1_hash = dsh(CTX_CORE, core1_cbor)
attest1_id = "comms.attest:" + multibase_z(core1_hash)

def sign(sk, by, role, signed_at, core_hash):
    # Plain Ed25519 over the raw canonical payload bytes. No outer hash:
    # domain separation lives inside the signed bytes (t: "comms.sig/1"),
    # plain Ed25519 keeps its collision resilience, and the payload is
    # signable by WebCrypto, ssh-agent, hardware keys, and stock libraries.
    payload = canon({"t": "comms.sig/1", "core": core_hash,
                     "by": by, "alg": "ed25519", "role": role,
                     "signed_at": signed_at})
    sig = sk.sign(payload).signature
    return payload, sig

p1, s1 = sign(sk_a, id_a, "author", "2026-06-11T00:00:01Z", core1_hash)

# verify round-trip
from nacl.signing import VerifyKey
VerifyKey(pub_a).verify(p1, s1)

# --- vector 2: same claim, two signatures (author + witness) — same ID ---------
p2, s2 = sign(sk_w, id_w, "witness", "2026-06-11T00:00:02Z", core1_hash)
VerifyKey(pub_w).verify(p2, s2)

# --- vector 3: endorsement/1 referencing vector 1 -------------------------------
core3 = {
    "v": 1,
    "t": "comms.attestation/1",
    "c": {"t": "endorsement/1", "target": attest1_id,
          "in_capacity": "field-records", "weight": "primary"},
    "f": {"issued_at": "2026-06-11T01:00:00Z", "language": "en"},
    "r": [{"role": "responds-to", "id": attest1_id}],
}
core3_cbor = canon(core3)
core3_hash = dsh(CTX_CORE, core3_cbor)
attest3_id = "comms.attest:" + multibase_z(core3_hash)
p3, s3 = sign(sk_w, id_w, "author", "2026-06-11T01:00:01Z", core3_hash)

vectors = {
    "scheme": {
        "hash": "blake3-256, domain separated: H(ctx,data)=blake3(uint8(len(ctx))||ctx||data)",
        "contexts": {"core": "comms.attest.core/1"},
        "signature": "plain Ed25519 over the raw canonical_cbor(sig_payload) bytes; no prehash",
        "sig_payload_fields": ["t", "core", "by", "alg", "role", "signed_at"],
        "cbor": "RFC 8949 4.2.1 core deterministic encoding",
    },
    "keys": [
        {"name": "author", "ed25519_seed_hex": "01" * 32,
         "public_key_hex": pub_a.hex(), "steward_id": id_a},
        {"name": "witness", "ed25519_seed_hex": "02" * 32,
         "public_key_hex": pub_w.hex(), "steward_id": id_w},
    ],
    "vectors": [
        {"name": "general-claim, single author signature",
         "core": {**core1, "c": {**core1["c"], "content": {
             "media_type": core1["c"]["content"]["media_type"],
             "body_utf8": core1["c"]["content"]["body"].decode()}}},
         "canonical_core_cbor_hex": core1_cbor.hex(),
         "core_hash_hex": core1_hash.hex(),
         "attestation_id": attest1_id,
         "signatures": [
             {"by": id_a, "role": "author", "signed_at": "2026-06-11T00:00:01Z",
              "sig_payload_cbor_hex": p1.hex(), "signature_hex": s1.hex()}]},
        {"name": "same core, added witness signature (ID must not change)",
         "attestation_id": attest1_id,
         "signatures_added": [
             {"by": id_w, "role": "witness", "signed_at": "2026-06-11T00:00:02Z",
              "sig_payload_cbor_hex": p2.hex(), "signature_hex": s2.hex()}]},
        {"name": "endorsement referencing vector 1",
         "core": core3,
         "canonical_core_cbor_hex": core3_cbor.hex(),
         "core_hash_hex": core3_hash.hex(),
         "attestation_id": attest3_id,
         "signatures": [
             {"by": id_w, "role": "author", "signed_at": "2026-06-11T01:00:01Z",
              "sig_payload_cbor_hex": p3.hex(), "signature_hex": s3.hex()}]},
    ],
    "negative_vectors": [
        {"name": "role swap must fail",
         "description": "Take vector 1's signature and present it with role='sponsor'. "
                        "Verification MUST fail because role is inside the signed payload."},
        {"name": "cross-context replay must fail",
         "description": "A blake3 hash of the core computed WITHOUT the domain "
                        "separation prefix must not verify as a core hash."},
    ],
}

with open(OUT_DIR / "attest-1.0-test-vectors.json", "w") as f:
    json.dump(vectors, f, indent=2)

print("attestation 1 id:", attest1_id)
print("attestation 3 id:", attest3_id)
print("core1 cbor bytes:", len(core1_cbor))
print("ok")

# ============================================================================
# Bundle vectors -- Attest 1.0 "Sneakernet bundle format" + Amendment A1.8.
# Derived independently here, like everything above: a second implementation
# is conformant only if it reproduces these bytes and rejects the negatives.
# ============================================================================

CTX_BUNDLE = b"comms.bundle/1"
SEAL_TAG = "comms.bundle.seal/1"


def envelope(core, sigs):
    return {**core, "s": sigs}


def sigobj(by, role, signed_at, signature):
    return {"by": by, "alg": "ed25519", "role": role,
            "signed_at": signed_at, "signature": signature}


# The two members are exactly attest vectors 1 and 3, carried as full
# envelopes -- a Rust impl that passes the attest vectors gets them for free.
env1 = envelope(core1, [sigobj(id_a, "author", "2026-06-11T00:00:01Z", s1)])
env3 = envelope(core3, [sigobj(id_w, "author", "2026-06-11T01:00:01Z", s3)])

# The A1.8 seal: a signed general-claim/1 enumerating the member ids and
# binding them with H("comms.bundle/1", canon(manifest)).
seal_created_at = "2026-06-11T02:00:00Z"
seal_signed_at = "2026-06-11T02:00:01Z"
member_ids_sorted = sorted([attest1_id, attest3_id])
seal_manifest = {
    "created_at": seal_created_at,
    "created_by": id_a,
    "description": "two field notes",
    "attestation_ids": member_ids_sorted,
}
seal_manifest_cbor = canon(seal_manifest)
bundle_hash = dsh(CTX_BUNDLE, seal_manifest_cbor)
seal_body = canon({"t": SEAL_TAG, "manifest": seal_manifest,
                   "bundle_hash": bundle_hash})
seal_core = {
    "v": 1, "t": "comms.attestation/1",
    "c": {"t": "general-claim/1", "about": "comms.bundle", "kind": "synthesis",
          "content": {"media_type": "application/cbor", "body": seal_body},
          "support": []},
    "f": {"issued_at": seal_created_at, "language": "zxx",
          "occasion": "bundle seal (A1.8)"},
    "r": [],
}
seal_core_cbor = canon(seal_core)
seal_core_hash = dsh(CTX_CORE, seal_core_cbor)
seal_id = "comms.attest:" + multibase_z(seal_core_hash)
seal_payload, seal_sig = sign(sk_a, id_a, "author", seal_signed_at, seal_core_hash)
VerifyKey(pub_a).verify(seal_payload, seal_sig)
env_seal = envelope(seal_core, [sigobj(id_a, "author", seal_signed_at, seal_sig)])

# Positive bundle: two members + the seal.
bundle_pos = {"v": 1, "t": "comms.bundle/1",
              "attestations": [env1, env3, env_seal]}
bundle_pos_cbor = canon(bundle_pos)

# Media-key rule: multibase base58btc of RAW blake3-256 (content addressing,
# NOT domain separated -- it names bytes, not a protocol object).
media_blob = "a small attached photograph of the northern field".encode()
media_key = multibase_z(blake3.blake3(media_blob).digest())

# Negative 1: a member dropped in transit; the seal still lists it.
bundle_drop = {"v": 1, "t": "comms.bundle/1", "attestations": [env3, env_seal]}
bundle_drop_cbor = canon(bundle_drop)

# Negative 2: an unsealed outsider attestation smuggled in.
sk_c, pub_c, id_c = steward(0x03)
forged_core = {
    "v": 1, "t": "comms.attestation/1",
    "c": {"t": "general-claim/1", "about": id_w, "kind": "observation",
          "content": {"media_type": "text/plain;charset=utf-8",
                      "body": "the well was poisoned".encode()},
          "support": []},
    "f": {"issued_at": "2026-06-11T03:00:00Z", "language": "en"},
    "r": [],
}
forged_cbor = canon(forged_core)
forged_hash = dsh(CTX_CORE, forged_cbor)
forged_id = "comms.attest:" + multibase_z(forged_hash)
_, forged_sig = sign(sk_c, id_c, "author", "2026-06-11T03:00:01Z", forged_hash)
env_forged = envelope(forged_core,
                      [sigobj(id_c, "author", "2026-06-11T03:00:01Z", forged_sig)])
bundle_smuggle = {"v": 1, "t": "comms.bundle/1",
                  "attestations": [env1, env3, env_seal, env_forged]}
bundle_smuggle_cbor = canon(bundle_smuggle)


def _seal_core_json(core):
    """JSON projection: the seal body is bytes on the wire, hex in the file."""
    return {**core, "c": {**core["c"], "content": {
        "media_type": core["c"]["content"]["media_type"],
        "body_hex": core["c"]["content"]["body"].hex()}}}


bundle_vectors = {
    "scheme": {
        "bundle_type": "comms.bundle/1",
        "container": "CBOR map {v:1, t:'comms.bundle/1', attestations:[envelope,...], "
                     "media?:{key:bytes}, manifest?:{created_at,created_by,description}}",
        "seal": "A1.8: a signed general-claim/1 (media_type application/cbor) "
                "carrying canon({t:'comms.bundle.seal/1', manifest, bundle_hash}), "
                "itself a member of the bundle",
        "seal_tag": SEAL_TAG,
        "bundle_hash": "H('comms.bundle/1', canon(manifest)), A1.1 domain-separated blake3",
        "manifest_fields": ["created_at", "created_by", "description", "attestation_ids"],
        "attestation_ids": "ids of the NON-seal members, sorted ascending as strings",
        "media_key": "multibase base58btc ('z') of RAW blake3-256 of the blob "
                     "(content addressing; NOT domain separated)",
        "members_match": "the seal verifies iff its id set == the present non-seal id set",
    },
    "keys": [
        {"name": "author", "ed25519_seed_hex": "01" * 32,
         "public_key_hex": pub_a.hex(), "steward_id": id_a},
        {"name": "witness", "ed25519_seed_hex": "02" * 32,
         "public_key_hex": pub_w.hex(), "steward_id": id_w},
        {"name": "outsider", "ed25519_seed_hex": "03" * 32,
         "public_key_hex": pub_c.hex(), "steward_id": id_c},
    ],
    "members": [
        {"attestation_id": attest1_id, "note": "attest vector 1 (general-claim)"},
        {"attestation_id": attest3_id, "note": "attest vector 3 (endorsement)"},
    ],
    "seal": {
        "manifest": seal_manifest,
        "manifest_canonical_cbor_hex": seal_manifest_cbor.hex(),
        "bundle_hash_hex": bundle_hash.hex(),
        "seal_body_cbor_hex": seal_body.hex(),
        "core": _seal_core_json(seal_core),
        "canonical_core_cbor_hex": seal_core_cbor.hex(),
        "core_hash_hex": seal_core_hash.hex(),
        "attestation_id": seal_id,
        "signature": {"by": id_a, "role": "author", "signed_at": seal_signed_at,
                      "sig_payload_cbor_hex": seal_payload.hex(),
                      "signature_hex": seal_sig.hex()},
    },
    "bundle": {
        "description": "two members + A1.8 seal",
        "member_ids": member_ids_sorted,
        "seal_id": seal_id,
        "canonical_cbor_hex": bundle_pos_cbor.hex(),
        "expect": {"seal_ok": True, "sealed_by": id_a, "missing": [], "extra": []},
    },
    "media_key_example": {"body_utf8": media_blob.decode(), "media_key": media_key},
    "negative_vectors": [
        {"name": "dropped member must fail the seal",
         "description": "the envelope for member 1 is removed; the seal still lists it",
         "canonical_cbor_hex": bundle_drop_cbor.hex(),
         "expect": {"seal_ok": False, "missing": [attest1_id], "extra": []}},
        {"name": "smuggled member must fail the seal",
         "description": "an unsealed outsider attestation is appended",
         "canonical_cbor_hex": bundle_smuggle_cbor.hex(),
         "forged_id": forged_id,
         "expect": {"seal_ok": False, "missing": [], "extra": [forged_id]}},
    ],
}

with open(OUT_DIR / "attest-1.0-bundle-vectors.json", "w") as f:
    json.dump(bundle_vectors, f, indent=2)

print("bundle hash:", bundle_hash.hex()[:16], "...")
print("seal id:", seal_id)
print("bundle cbor bytes:", len(bundle_pos_cbor))
print("bundle vectors ok")
