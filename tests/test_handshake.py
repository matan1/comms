"""Behavioral tests for the identity handshake: challenge / bound response /
verify, per docs/sentira-motes-identity-handshake.1.0.md. The acceptance
criteria from that doc's "minimal first step" are the spine here: the same
identity is recognized across a gap, a replayed handshake is rejected, and
no record claims authority it was not granted.
"""

import cbor2
import pytest

import comms
from comms.handshake import (NonceLedger, bind_response, verify_bound_response,
                             mutual_handshake, broker_record, _now)


@pytest.fixture
def sim_host():
    return comms.Steward.generate("sim-host")


@pytest.fixture
def session():
    return comms.Steward.generate("sentira-session")


@pytest.fixture
def tracker():
    return comms.Steward.generate("tracker-host")


PURPOSE = "attach: mote-world/meadow, scene-profile basic"


# ---- the happy path -------------------------------------------------------------

def test_bound_response_verifies(sim_host, session):
    ledger = NonceLedger()
    nonce = ledger.issue()
    att = bind_response(sim_host, nonce=nonce, peer=session.id, purpose=PURPOSE)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id,
        purpose=PURPOSE, ledger=ledger)
    assert ok, why


def test_mutual_handshake_both_directions(sim_host, session):
    result = mutual_handshake(session, sim_host, purpose=PURPOSE)
    assert result["ok"]
    assert result["a_verified_b"][0] and result["b_verified_a"][0]
    # Each response is signed by its own author in the party role.
    assert result["response_a"].signed_by(session.id)
    assert result["response_b"].signed_by(sim_host.id)


def test_same_identity_recognized_across_restart(sim_host, session, tmp_path):
    """The doc's acceptance check: restart a sim-host (key persisted and
    reloaded) and confirm a session recognizes the same identity."""
    sim_host.save(tmp_path / "sim-host.json")
    restarted = comms.Steward.load(tmp_path / "sim-host.json")
    assert restarted.id == sim_host.id

    ledger = NonceLedger()
    nonce = ledger.issue()
    att = bind_response(restarted, nonce=nonce, peer=session.id, purpose=PURPOSE)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id,
        ledger=ledger)
    assert ok, why


# ---- what must fail -------------------------------------------------------------

def test_replayed_response_is_rejected(sim_host, session):
    ledger = NonceLedger()
    nonce = ledger.issue()
    att = bind_response(sim_host, nonce=nonce, peer=session.id, purpose=PURPOSE)
    ok, _ = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id, ledger=ledger)
    assert ok
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id, ledger=ledger)
    assert not ok and "replay" in why


def test_unissued_nonce_is_rejected(sim_host, session):
    ledger = NonceLedger()
    att = bind_response(sim_host, nonce="feedfacefeedface", peer=session.id,
                        purpose=PURPOSE)
    ok, why = verify_bound_response(
        att, nonce="feedfacefeedface", responder=sim_host.id,
        verifier=session.id, ledger=ledger)
    assert not ok and "unknown nonce" in why


def test_nonce_mismatch_is_rejected(sim_host, session):
    att = bind_response(sim_host, nonce="aaaa", peer=session.id, purpose=PURPOSE)
    ok, why = verify_bound_response(
        att, nonce="bbbb", responder=sim_host.id, verifier=session.id)
    assert not ok and "nonce" in why


def test_response_bound_to_other_peer_is_rejected(sim_host, session):
    """Peer binding: a response addressed to Carol must not satisfy Bob."""
    carol = comms.Steward.generate("carol")
    nonce = "cafe" * 4
    att = bind_response(sim_host, nonce=nonce, peer=carol.id, purpose=PURPOSE)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id)
    assert not ok and "different peer" in why


def test_imposter_cannot_speak_for_responder(session):
    """A body naming the real sim-host but signed by another key must fail:
    key possession is the entire point."""
    real = comms.Steward.generate("real-sim-host")
    imposter = comms.Steward.generate("imposter")
    nonce = "beef" * 4
    att = bind_response(imposter, nonce=nonce, peer=session.id, purpose=PURPOSE)
    # Claim to be the real host in the body.
    from comms.canonical import canonical_cbor
    body = cbor2.loads(att.claim["content"]["body"])
    body["responder"] = real.id
    att.claim["content"]["body"] = canonical_cbor(body)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=real.id, verifier=session.id)
    assert not ok


def test_tampered_body_breaks_signature(sim_host, session):
    nonce = "d00d" * 4
    att = bind_response(sim_host, nonce=nonce, peer=session.id, purpose=PURPOSE)
    body = cbor2.loads(att.claim["content"]["body"])
    body["purpose"] = "attach: mote-world/meadow, scene-profile FULL-CONTROL"
    from comms.canonical import canonical_cbor
    att.claim["content"]["body"] = canonical_cbor(body)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id)
    assert not ok and "signature" in why


def test_expired_response_is_rejected(sim_host, session):
    nonce = "aged" * 4
    att = bind_response(sim_host, nonce=nonce, peer=session.id, purpose=PURPOSE,
                        now="2026-07-06T00:00:00Z", ttl=60)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id,
        now="2026-07-06T00:02:00Z")
    assert not ok and "expired" in why
    # But within the window it verifies.
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id,
        now="2026-07-06T00:00:30Z")
    assert ok, why


def test_purpose_mismatch_is_rejected(sim_host, session):
    nonce = "aims" * 4
    att = bind_response(sim_host, nonce=nonce, peer=session.id, purpose=PURPOSE)
    ok, why = verify_bound_response(
        att, nonce=nonce, responder=sim_host.id, verifier=session.id,
        purpose="attach: a different world entirely")
    assert not ok and "purpose" in why


# ---- the broker's record ---------------------------------------------------------

def test_broker_record_is_custody_not_authorization(sim_host, session, tracker):
    result = mutual_handshake(session, sim_host, purpose=PURPOSE)
    record = broker_record(tracker, result["response_a"], result["response_b"])

    ok, why = record.verified()
    assert ok, why
    assert record.signed_by(tracker.id)
    assert all(s["role"] == "custodian" for s in record.signatures)
    # It references both bound responses...
    assert record.claim["support"] == [result["response_a"].id,
                                       result["response_b"].id]
    ref_ids = {r["id"] for r in record.refs}
    assert ref_ids == {result["response_a"].id, result["response_b"].id}
    # ...and claims arrangement only: no grant, no authority vocabulary.
    assert record.claim["kind"] == "brokered-pairing"
    body = record.claim["content"]["body"].decode()
    assert "not authorization" in body


def test_handshake_grants_nothing(sim_host, session):
    """A passed handshake is evidence, not admission: nothing in the record
    carries an authority, capability, or membership field."""
    result = mutual_handshake(session, sim_host, purpose=PURPOSE)
    for att in (result["response_a"], result["response_b"]):
        assert att.claim["t"] == "general-claim/1"
        body = cbor2.loads(att.claim["content"]["body"])
        assert set(body) == {"nonce", "responder", "peer", "purpose", "expires_at"}
