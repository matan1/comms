"""Behavioral tests for the comms toolkit: envelopes, layered validation,
stores, and ceremonies under the A1 signing scheme.
"""

import re

import pytest

import comms
from comms.attest import _now


@pytest.fixture
def alice():
    return comms.Steward.generate("alice")


@pytest.fixture
def bob():
    return comms.Steward.generate("bob")


def make_att(signer, **kwargs):
    claim = comms.claims.general_claim(
        about=signer.id, kind="observation", body="the well is clear")
    return comms.Attestation.build(claim, **kwargs).sign(signer)


# ---- envelope basics ------------------------------------------------------------

def test_roundtrip_preserves_id_and_signatures(alice):
    att = make_att(alice, community="testground")
    back = comms.Attestation.from_cbor(att.to_cbor())
    assert back.id == att.id
    assert back.verified() == (True, "ok")
    assert back.signed_by(alice.id)


def test_tamper_changes_id_and_breaks_signature(alice):
    att = make_att(alice)
    env = comms.Attestation.from_cbor(att.to_cbor()).to_envelope()
    env["c"]["content"]["body"] = b"the well is poisoned"
    tampered = comms.Attestation.from_envelope(env)
    assert tampered.id != att.id
    assert not tampered.signatures_valid()


def test_build_timestamps_are_canonical(alice):
    att = make_att(alice)
    pin = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
    assert pin.match(att.frame["issued_at"])
    assert pin.match(att.signatures[0]["signed_at"])
    assert pin.match(_now())


def test_signing_does_not_change_id(alice, bob):
    att = make_att(alice)
    id_one = att.id
    att.sign(bob, role="witness")
    assert att.id == id_one
    assert att.signatures_valid()
    assert att.signers() == {alice.id, bob.id}


def test_general_claim_body_str_and_bytes_agree():
    a = comms.claims.general_claim(about="x", kind="other", body="same words")
    b = comms.claims.general_claim(about="x", kind="other", body=b"same words")
    assert a == b
    assert isinstance(a["content"]["body"], bytes)


# ---- validation layers (A1.4) ----------------------------------------------------

def test_unresolved_ref_is_awaiting_context_not_malformed(alice):
    missing = "comms.attest:z" + "1" * 44
    claim = comms.claims.endorsement(target=missing, in_capacity="testing")
    att = comms.Attestation.build(
        claim, refs=[{"role": "responds-to", "id": missing}]).sign(alice)
    assert att.structurally_valid() == (True, "ok")
    assert att.verified() == (True, "ok")
    ok, unresolved = att.resolvable(comms.Store())
    assert not ok and unresolved == [missing]


def test_refs_resolve_once_target_in_store(alice):
    store = comms.Store()
    target = make_att(alice)
    store.put(target)
    follow = comms.Attestation.build(
        comms.claims.endorsement(target=target.id, in_capacity="testing"),
        refs=[{"role": "responds-to", "id": target.id}]).sign(alice)
    assert follow.resolvable(store) == (True, [])
    assert follow.verify_well_formed(store) == (True, "ok")


def test_malformed_ref_is_structural(alice):
    att = make_att(alice)
    att.refs.append({"role": "context", "id": "not-an-attest-id"})
    ok, why = att.structurally_valid()
    assert not ok and "ref id" in why


# ---- store ------------------------------------------------------------------------

def test_directory_store_roundtrip(tmp_path, alice):
    att = make_att(alice)
    store = comms.Store(tmp_path)
    store.put(att)
    reloaded = comms.Store(tmp_path)
    assert len(reloaded) == 1
    got = reloaded.get(att.id)
    assert got is not None and got.verified() == (True, "ok")


def test_store_referencing(alice):
    store = comms.Store()
    target = make_att(alice)
    store.put(target)
    follow = comms.Attestation.build(
        comms.claims.endorsement(target=target.id, in_capacity="testing"),
        refs=[{"role": "responds-to", "id": target.id}]).sign(alice)
    store.put(follow)
    assert [a.id for a in store.referencing(target.id, "responds-to")] == [follow.id]


# ---- ceremonies under A1 signing ---------------------------------------------------

def test_capability_rite_attestations_verify(alice):
    store = comms.Store()
    net = comms.Network(store, comms.Steward.generate("village"))
    agent = comms.Steward.generate("newcomer")
    res = net.capability_rite(alice, agent, "compute",
                              {"type": "compute", "difficulty": 8})
    assert res["ok"], res
    for att in store.all():
        assert att.verified() == (True, "ok")
    proof = store.get(res["proof"])
    assert proof.resolvable(store) == (True, [])


def test_admission_binds_roles(alice):
    store = comms.Store()
    community = comms.Steward.generate("village")
    net = comms.Network(store, community)
    agent = comms.Steward.generate("newcomer")
    prov = net.provenance_rite(agent, alice, kind="service", model_id="m-1")
    binding_id = net.admit(alice, agent, role="member", authority=[prov])
    binding = store.get(binding_id)
    assert binding.verified() == (True, "ok")
    roles = {s["role"]: s["by"] for s in binding.signatures}
    assert roles == {"sponsor": alice.id, "community": community.id}
    assert binding.resolvable(store) == (True, [])


def test_recognition_rite_both_sign(alice, bob):
    store = comms.Store()
    net = comms.Network(store, comms.Steward.generate("village"))
    rec = store.get(net.recognition_rite(alice, bob, prior=[], note="harvest"))
    assert rec.verified() == (True, "ok")
    assert rec.signers() == {alice.id, bob.id}
