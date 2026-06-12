"""Golden-vector conformance for Attest 1.0 + Amendment A1 (data/attest-1.0-
test-vectors.json), checked with the independent verifier in tests/a1.py.

A1.9: a second implementation is conformant with the encoding rules only if
it reproduces all canonical CBOR hex, hashes, identifiers, and signature
payloads byte-for-byte, and rejects both negative vectors.
"""

import blake3
import pytest
from nacl.signing import SigningKey, VerifyKey
from nacl.exceptions import BadSignatureError

import a1


def wire_core(vector_core: dict) -> dict:
    """The JSON projection renders textual bodies as body_utf8; the wire form
    carries body as bytes (A1.6)."""
    core = {**vector_core, "c": {**vector_core["c"]}}
    content = core["c"].get("content")
    if content and "body_utf8" in content:
        core["c"]["content"] = {
            "media_type": content["media_type"],
            "body": content["body_utf8"].encode("utf-8"),
        }
    return core


def full_sig(sig: dict, alg: str = "ed25519") -> dict:
    """A vector signature object in wire form (alg implied by the scheme)."""
    return {"by": sig["by"], "alg": alg, "role": sig["role"],
            "signed_at": sig["signed_at"],
            "signature": bytes.fromhex(sig["signature_hex"])}


def test_published_keys(attest_vectors):
    for k in attest_vectors["keys"]:
        sk = SigningKey(bytes.fromhex(k["ed25519_seed_hex"]))
        pub = sk.verify_key.encode()
        assert pub.hex() == k["public_key_hex"]
        assert a1.personal_steward_id(pub) == k["steward_id"]
        assert a1.pub_from_steward_id(k["steward_id"]) == pub


@pytest.mark.parametrize("idx", [0, 2])
def test_canonical_encoding_hash_and_id(attest_vectors, idx):
    v = attest_vectors["vectors"][idx]
    core = wire_core(v["core"])
    encoded = a1.canon(core)
    assert encoded.hex() == v["canonical_core_cbor_hex"], "canonical CBOR"
    assert a1.dsh(a1.CTX_CORE, encoded).hex() == v["core_hash_hex"], "core hash"
    assert a1.attest_id(core) == v["attestation_id"], "attestation id"


@pytest.mark.parametrize("idx", [0, 2])
def test_canonical_form_roundtrips(attest_vectors, idx):
    import cbor2
    v = attest_vectors["vectors"][idx]
    raw = bytes.fromhex(v["canonical_core_cbor_hex"])
    assert a1.canon(cbor2.loads(raw)) == raw


def test_signature_payloads_and_signatures(attest_vectors):
    for v in attest_vectors["vectors"]:
        core = wire_core(v["core"]) if "core" in v else wire_core(
            attest_vectors["vectors"][0]["core"])
        for sig in v.get("signatures", []) + v.get("signatures_added", []):
            payload = a1.sig_payload(core, full_sig(sig))
            assert payload.hex() == sig["sig_payload_cbor_hex"], "payload bytes"
            assert a1.verify_personal_signature(core, full_sig(sig)), \
                f"signature by {sig['by']} ({sig['role']})"


def test_id_is_signature_independent(attest_vectors):
    v1, v2 = attest_vectors["vectors"][0], attest_vectors["vectors"][1]
    assert v2["attestation_id"] == v1["attestation_id"]


def test_signing_reproducible_from_seed(attest_vectors):
    """Ed25519 is deterministic: signing the vector payload with the published
    seed must reproduce the published signature exactly."""
    v = attest_vectors["vectors"][0]
    sig = v["signatures"][0]
    seed = bytes.fromhex(attest_vectors["keys"][0]["ed25519_seed_hex"])
    produced = SigningKey(seed).sign(
        bytes.fromhex(sig["sig_payload_cbor_hex"])).signature
    assert produced.hex() == sig["signature_hex"]


# ---- negative vectors (A1.9: every implementation must fail these) -------------

def test_negative_role_swap_fails(attest_vectors):
    """A witness/author signature re-presented under another role must not
    verify: role is inside the signed payload (A1.3)."""
    v = attest_vectors["vectors"][0]
    core = wire_core(v["core"])
    swapped = full_sig(v["signatures"][0])
    swapped["role"] = "sponsor"
    assert not a1.verify_personal_signature(core, swapped)


def test_negative_cross_context_replay_fails(attest_vectors):
    """An un-prefixed blake3 core hash must not be accepted (A1.1): a payload
    built over the raw hash differs, so the published signature fails it."""
    v = attest_vectors["vectors"][0]
    core = wire_core(v["core"])
    sig = v["signatures"][0]
    raw_hash = blake3.blake3(a1.canon(core)).digest()
    assert raw_hash.hex() != v["core_hash_hex"]
    forged_payload = a1.canon({
        "t": "comms.sig/1", "core": raw_hash, "by": sig["by"],
        "alg": "ed25519", "role": sig["role"], "signed_at": sig["signed_at"],
    })
    pub = a1.pub_from_steward_id(sig["by"])
    with pytest.raises(BadSignatureError):
        VerifyKey(pub).verify(forged_payload, bytes.fromhex(sig["signature_hex"]))
