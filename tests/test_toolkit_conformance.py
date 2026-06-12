"""The comms toolkit (canonical.py / attest.py / identity.py) must conform to
the Attest 1.0 + A1 golden vectors: reproduce identifiers and signatures
byte-for-byte and reject the negative vectors.
"""

import cbor2
import pytest

import comms
from comms.canonical import canonical_cbor, core_hash, dsh, CTX_CORE

from test_attest_vectors import wire_core, full_sig


def steward_from_vector(attest_vectors, name) -> comms.Steward:
    import nacl.signing
    k = next(k for k in attest_vectors["keys"] if k["name"] == name)
    sk = nacl.signing.SigningKey(bytes.fromhex(k["ed25519_seed_hex"]))
    return comms.Steward(sk, label=name)


def att_from_vector(v) -> comms.Attestation:
    core = wire_core(v["core"])
    return comms.Attestation(claim=core["c"], frame=core["f"], refs=core["r"],
                             signatures=[full_sig(s) for s in v["signatures"]])


def test_steward_ids_match_vectors(attest_vectors):
    for k in attest_vectors["keys"]:
        s = steward_from_vector(attest_vectors, k["name"])
        assert s.id == k["steward_id"]
        assert s.pubkey.hex() == k["public_key_hex"]


@pytest.mark.parametrize("idx", [0, 2])
def test_toolkit_reproduces_encoding_hash_and_id(attest_vectors, idx):
    v = attest_vectors["vectors"][idx]
    att = att_from_vector(v)
    assert canonical_cbor(att.core()).hex() == v["canonical_core_cbor_hex"]
    assert core_hash(att.core()).hex() == v["core_hash_hex"]
    assert att.id == v["attestation_id"]


@pytest.mark.parametrize("idx", [0, 2])
def test_toolkit_verifies_vector_signatures(attest_vectors, idx):
    att = att_from_vector(attest_vectors["vectors"][idx])
    assert att.signatures_valid()
    ok, why = att.verified()
    assert ok, why


def test_toolkit_signing_reproduces_vector_signature(attest_vectors):
    """Signing the vector core with the published seed and the vector's
    signed_at must reproduce the published signature bytes exactly."""
    v = attest_vectors["vectors"][0]
    expected = v["signatures"][0]
    att = comms.Attestation(claim=wire_core(v["core"])["c"],
                            frame=wire_core(v["core"])["f"], refs=[])
    att.sign(steward_from_vector(attest_vectors, "author"),
             role="author", signed_at=expected["signed_at"])
    produced = att.signatures[0]
    assert produced["signature"].hex() == expected["signature_hex"]
    assert att.id == v["attestation_id"]


def test_toolkit_id_signature_independent(attest_vectors):
    v1, v2 = attest_vectors["vectors"][0], attest_vectors["vectors"][1]
    att = att_from_vector(v1)
    id_before = att.id
    att.signatures.append(full_sig(v2["signatures_added"][0]))
    assert att.id == id_before == v2["attestation_id"]
    assert att.signatures_valid()


def test_general_claim_constructor_matches_vector(attest_vectors):
    """claims.general_claim with a str body must produce the vector's wire
    claim exactly (A1.6: body is bytes on the wire)."""
    v = attest_vectors["vectors"][0]
    claim = comms.claims.general_claim(
        about=attest_vectors["keys"][1]["steward_id"],
        kind="observation",
        body="The northern field was sown on the new moon.",
        media_type="text/plain;charset=utf-8")
    assert claim == wire_core(v["core"])["c"]


# ---- negative vectors -----------------------------------------------------------

def test_toolkit_rejects_role_swap(attest_vectors):
    v = attest_vectors["vectors"][0]
    att = att_from_vector(v)
    att.signatures[0]["role"] = "sponsor"
    assert not att.signatures_valid()
    assert att.verified() == (False, "signature verification failed")


def test_toolkit_rejects_cross_context_replay(attest_vectors):
    """A signature over an un-prefixed blake3 core hash (the pre-A1 scheme)
    must not verify."""
    import blake3
    v = attest_vectors["vectors"][0]
    att = att_from_vector(v)
    author = steward_from_vector(attest_vectors, "author")
    raw_hash = blake3.blake3(canonical_cbor(att.core())).digest()
    assert raw_hash != core_hash(att.core())
    legacy = {"by": author.id, "alg": "ed25519", "role": "author",
              "signed_at": "2026-06-11T00:00:01Z",
              "signature": author.sign(raw_hash)}
    att.signatures = [legacy]
    assert not att.signatures_valid()


def test_toolkit_rejects_duplicate_signature_objects(attest_vectors):
    att = att_from_vector(attest_vectors["vectors"][0])
    att.signatures.append(dict(att.signatures[0]))
    assert att.structurally_valid() == (False, "duplicate signature object")


def test_toolkit_rejects_unrecognized_alg(attest_vectors):
    att = att_from_vector(attest_vectors["vectors"][0])
    att.signatures[0]["alg"] = "ed25519ph"
    ok, why = att.structurally_valid()
    assert not ok and "unrecognized" in why
    assert not att.signatures_valid()


def test_toolkit_rejects_noncanonical_timestamps(attest_vectors):
    """A1.6: RFC 3339 UTC, Z suffix, no fractional seconds."""
    att = att_from_vector(attest_vectors["vectors"][0])
    for bad in ("2026-06-11T00:00:00.000Z", "2026-06-11T00:00:00+00:00",
                "2026-06-11 00:00:00Z"):
        att.frame = {**att.frame, "issued_at": bad}
        ok, why = att.structurally_valid()
        assert not ok, bad
