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
import cbor2
import blake3
from nacl.signing import SigningKey

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

with open("/home/claude/attest-1.0-test-vectors.json", "w") as f:
    json.dump(vectors, f, indent=2)

print("attestation 1 id:", attest1_id)
print("attestation 3 id:", attest3_id)
print("core1 cbor bytes:", len(core1_cbor))
print("ok")
