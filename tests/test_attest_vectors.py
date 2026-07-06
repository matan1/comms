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
    """The JSON projection renders textual bodies as body_utf8 and detached
    hashes as body_b3_hex; the wire form carries bytes (A1.6, A2.1)."""
    core = {**vector_core, "c": {**vector_core["c"]}}
    content = core["c"].get("content")
    if content and "body_utf8" in content:
        core["c"]["content"] = {
            "media_type": content["media_type"],
            "body": content["body_utf8"].encode("utf-8"),
        }
    elif content and "body_b3_hex" in content:
        core["c"]["content"] = {
            "media_type": content["media_type"],
            "body_b3": bytes.fromhex(content["body_b3_hex"]),
            "body_len": content["body_len"],
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


@pytest.mark.parametrize("idx", [0, 2, 3])
def test_canonical_encoding_hash_and_id(attest_vectors, idx):
    v = attest_vectors["vectors"][idx]
    core = wire_core(v["core"])
    encoded = a1.canon(core)
    assert encoded.hex() == v["canonical_core_cbor_hex"], "canonical CBOR"
    assert a1.dsh(a1.CTX_CORE, encoded).hex() == v["core_hash_hex"], "core hash"
    assert a1.attest_id(core) == v["attestation_id"], "attestation id"


@pytest.mark.parametrize("idx", [0, 2, 3])
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


# ---- Amendment A2: detached bodies ------------------------------------------

def _a2_negative(attest_vectors, name_part: str) -> dict:
    return next(n for n in attest_vectors["negative_vectors"]
                if name_part in n["name"])


def test_a2_detached_vector_verifies_and_commits(attest_vectors):
    """Vector 4's commitment is the plain blake3 of the published body, and the
    reference implementation judges body status per A2.2."""
    from comms.attest import Attestation, body_status
    v = attest_vectors["vectors"][3]
    core = wire_core(v["core"])
    body = v["body_utf8"].encode("utf-8")
    assert blake3.blake3(body).digest().hex() == v["body_b3_hex"]
    assert len(body) == v["body_len"]

    att = Attestation(claim=core["c"], frame=core["f"], refs=core["r"],
                      signatures=[full_sig(s) for s in v["signatures"]])
    ok, why = att.verified()
    assert ok, why
    content = att.claim["content"]
    assert body_status(content) == "absent"          # no bytes at hand: normal
    assert body_status(content, body) == "verified"


def test_a2_negative_both_forms_rejected(attest_vectors):
    """Exactly one of body/body_b3 (A2.1): both present fails layer 1."""
    import cbor2
    from comms.attest import Attestation
    n = _a2_negative(attest_vectors, "both body and body_b3")
    core = cbor2.loads(bytes.fromhex(n["canonical_core_cbor_hex"]))
    att = Attestation(claim=core["c"], frame=core["f"], refs=core["r"])
    ok, why = att.structurally_valid()
    assert not ok and "both" in why


def test_a2_negative_neither_form_rejected(attest_vectors):
    import cbor2
    from comms.attest import Attestation
    n = _a2_negative(attest_vectors, "neither body nor body_b3")
    core = cbor2.loads(bytes.fromhex(n["canonical_core_cbor_hex"]))
    att = Attestation(claim=core["c"], frame=core["f"], refs=core["r"])
    ok, why = att.structurally_valid()
    assert not ok and "neither" in why


def test_a2_mismatched_body_is_reported_not_fatal(attest_vectors):
    """Wrong bytes at hand: the attestation still verifies (the math holds),
    the body status says mismatched — separable judgments (A2.2)."""
    from comms.attest import Attestation, body_status
    v = attest_vectors["vectors"][3]
    n = _a2_negative(attest_vectors, "mismatched detached body")
    core = wire_core(v["core"])
    att = Attestation(claim=core["c"], frame=core["f"], refs=core["r"],
                      signatures=[full_sig(s) for s in v["signatures"]])
    ok, why = att.verified()
    assert ok, why
    wrong = n["wrong_body_utf8"].encode("utf-8")
    assert body_status(att.claim["content"], wrong) == "mismatched"
