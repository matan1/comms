import pytest

from comms import claims


def test_vouch_policy_constructor():
    purpose = {
        "purpose": "admission",
        "positive_types": ["action-record/1"],
        "negative_types": ["action-record/1"],
        "min_positive_issuers": 2,
        "min_negative_issuers": 2,
        "min_endorsers": 2,
        "issuer_cap": 1,
        "require_direct": 1,
        "propagation": {"enabled": 0, "max_depth": 2, "min_paths": 2},
    }
    got = claims.vouch_policy(
        community="comms.steward:zcommunity",
        name="careful admission",
        purposes=[purpose],
        anchors=["comms.steward:zanchor"],
    )
    assert got["t"] == "vouch-policy/1"
    assert got["purposes"] == [purpose]


def test_vouch_disposition_validates_state():
    assert claims.vouch_disposition(target="comms.attest:z1", state="inactive") == {
        "t": "vouch-disposition/1",
        "target": "comms.attest:z1",
        "state": "inactive",
    }
    with pytest.raises(ValueError):
        claims.vouch_disposition(target="comms.attest:z1", state="revoked")


def test_vouch_judgment_constructor():
    got = claims.vouch_judgment(
        subject="comms.steward:zsubject",
        purpose="admission",
        policy="comms.attest:zpolicy",
        as_of="2026-06-14T12:00:00Z",
        outcome="awaiting-context",
        store_view="comms.vouch.view:zview",
        engine="comms-core/0.1.0",
        evidence=[],
        unresolved=["comms.attest:zmissing"],
    )
    assert got["t"] == "vouch-judgment/1"
    assert got["unresolved"] == ["comms.attest:zmissing"]
