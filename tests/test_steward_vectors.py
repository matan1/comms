"""Golden-vector conformance for Steward 1.0 (data/steward-test-vectors.json),
checked with the independent verifier in tests/a1.py.

The published negative vectors record only outcomes, so the negative cases are
reconstructed here from the published seeds and re-executed.
"""

import cbor2
import pytest
from nacl.signing import SigningKey

import a1

# vector list positions in the golden file
K0, RULE, K1, RULE2, KS, SUCC = range(6)


def decode_att(vector: dict) -> dict:
    """A vector as {core, signatures} in wire form."""
    core = cbor2.loads(bytes.fromhex(vector["canonical_core_cbor_hex"]))
    sigs = []
    for s in vector["signatures"]:
        sig = dict(s)
        sig["signature"] = bytes.fromhex(sig["signature"])
        sigs.append(sig)
    return {"core": core, "signatures": sigs}


@pytest.fixture(scope="module")
def atts(steward_vectors):
    return [decode_att(v) for v in steward_vectors["vectors"]]


@pytest.fixture(scope="module")
def keys(steward_vectors):
    return {k["name"]: SigningKey(bytes.fromhex(k["ed25519_seed_hex"]))
            for k in steward_vectors["keys"]}


@pytest.fixture(scope="module")
def store(steward_vectors, atts):
    """All keyset/1 links by attestation id (both communities' geneses)."""
    ids = [v["attestation_id"] for v in steward_vectors["vectors"]]
    return {ids[i]: atts[i] for i in (K0, K1, KS)}


def test_published_keys(steward_vectors, keys):
    for k in steward_vectors["keys"]:
        pub = keys[k["name"]].verify_key.encode()
        assert pub.hex() == k["public_key_hex"]
        assert a1.personal_steward_id(pub) == k["personal_steward_id"]


def test_encoding_hash_and_id(steward_vectors, atts):
    for v, att in zip(steward_vectors["vectors"], atts):
        encoded = a1.canon(att["core"])
        assert encoded.hex() == v["canonical_core_cbor_hex"]
        assert a1.dsh(a1.CTX_CORE, encoded).hex() == v["core_hash_hex"]
        assert a1.attest_id(att["core"]) == v["attestation_id"]


def test_genesis_anchored_community_ids(steward_vectors, atts):
    assert a1.community_id(atts[K0]["core"]["c"]["descriptor"]) == \
        steward_vectors["community_id"]
    assert a1.community_id(atts[KS]["core"]["c"]["descriptor"]) == \
        steward_vectors["successor_community_id"]
    # rotation does NOT rename: K1's descriptor differs from genesis but the
    # community field still carries the genesis-anchored id
    assert a1.community_id(atts[K1]["core"]["c"]["descriptor"]) != \
        steward_vectors["community_id"]
    assert atts[K1]["core"]["c"]["community"] == steward_vectors["community_id"]


def test_descriptor_well_formed(atts):
    for i in (K0, K1, KS):
        d = atts[i]["core"]["c"]["descriptor"]
        member_keys = [m["key"] for m in d["members"]]
        assert member_keys == sorted(member_keys), "members sorted by key bytes"
        assert len(set(member_keys)) == len(member_keys), "members unique"
        assert 1 <= d["threshold"] <= len(member_keys)


def test_chain_and_community_signatures(steward_vectors, atts, store):
    cid = steward_vectors["community_id"]
    # genesis and rotation links establish their descriptors
    k1_id = steward_vectors["vectors"][K1]["attestation_id"]
    assert a1.verify_chain(cid, k1_id, store) == atts[K1]["core"]["c"]["descriptor"]
    # community-signed attestations verify under K0 and, through the chain,
    # under K1 even though a founding key departed
    assert a1.verify_community_attestation(atts[RULE], cid, store)
    assert a1.verify_community_attestation(atts[RULE2], cid, store)


def test_succession_attestation(steward_vectors, atts, store, keys):
    cid2 = steward_vectors["successor_community_id"]
    assert a1.verify_community_attestation(atts[SUCC], cid2, store)
    # the personal witness signatures are by Bea (current member of the broken
    # keyset) and Cy (departed founder, key intact)
    witnesses = [s for s in atts[SUCC]["signatures"] if s["alg"] == "ed25519"]
    by = {w["by"] for w in witnesses}
    assert by == {a1.personal_steward_id(keys["Bea"].verify_key.encode()),
                  a1.personal_steward_id(keys["Cy"].verify_key.encode())}
    for w in witnesses:
        assert a1.verify_personal_signature(atts[SUCC]["core"], w)


def test_tampered_core_fails(steward_vectors, atts, store):
    cid = steward_vectors["community_id"]
    tampered = {"core": {**atts[RULE]["core"],
                         "f": {**atts[RULE]["core"]["f"], "language": "fr"}},
                "signatures": atts[RULE]["signatures"]}
    assert not a1.verify_community_attestation(tampered, cid, store)


# ---- negative vectors, reconstructed from published seeds ----------------------

def k1_id(steward_vectors):
    return steward_vectors["vectors"][K1]["attestation_id"]


def test_negative_sub_threshold(steward_vectors, atts, store, keys):
    """One valid signature against threshold 2 must fail."""
    cid = steward_vectors["community_id"]
    sig = a1.sign_set(atts[RULE2]["core"], by=cid, role="community",
                      signed_at="2026-07-01T09:00:01Z",
                      keyset_attest_id=k1_id(steward_vectors),
                      signers=[keys["Bea"]])
    bad = {"core": atts[RULE2]["core"], "signatures": [sig]}
    assert not a1.verify_community_attestation(bad, cid, store)


def test_negative_departed_key_ignored(steward_vectors, atts, store, keys):
    """Cy departed in the K1 rotation: Cy's inner signature is ignored, and
    Bea alone is below threshold."""
    cid = steward_vectors["community_id"]
    sig = a1.sign_set(atts[RULE2]["core"], by=cid, role="community",
                      signed_at="2026-07-01T09:00:01Z",
                      keyset_attest_id=k1_id(steward_vectors),
                      signers=[keys["Cy"], keys["Bea"]])
    bad = {"core": atts[RULE2]["core"], "signatures": [sig]}
    assert not a1.verify_community_attestation(bad, cid, store)


def test_negative_hostile_takeover(steward_vectors, atts, store, keys):
    """A keyset claiming to supersede K0 but signed only by its own new keys
    must fail chain verification."""
    cid = steward_vectors["community_id"]
    k0_id = steward_vectors["vectors"][K0]["attestation_id"]
    desc_x = a1.keyset_descriptor(
        [keys["Eve"].verify_key.encode(), keys["Fen"].verify_key.encode()], 2)
    kx_core = {"v": 1, "t": "comms.attestation/1",
               "c": {"t": "keyset/1", "community": cid, "descriptor": desc_x},
               "f": {"issued_at": "2026-07-01T09:00:00Z", "language": "en"},
               "r": [{"role": "supersedes", "id": k0_id}]}
    kx_id = a1.attest_id(kx_core)
    kx_att = {"core": kx_core,
              "signatures": [a1.sign_set(kx_core, by=cid, role="community",
                                         signed_at="2026-07-01T09:00:01Z",
                                         keyset_attest_id=kx_id,
                                         signers=[keys["Eve"], keys["Fen"]])]}
    hostile_store = dict(store)
    hostile_store[kx_id] = kx_att
    with pytest.raises(ValueError):
        a1.verify_chain(cid, kx_id, hostile_store)


def test_negative_outcomes_recorded(steward_vectors):
    assert all(n["verified_fails"] for n in steward_vectors["negative_vectors"])
