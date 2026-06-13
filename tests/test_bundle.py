"""Sneakernet bundle tests (comms/bundle.py): round-trip, the A1.8 integrity
seal, tamper detection (removal / substitution), layered resolution, and
content-addressed media. Runnable under pytest or directly:

    python tests/test_bundle.py
"""

from __future__ import annotations

import tempfile
from pathlib import Path

from comms import Attestation, Steward, Store, claims
from comms import bundle as B


def _fixture():
    """Two stewards; a1 by alice, a2 by bob referencing a1 as context."""
    alice = Steward.generate(label="alice")
    bob = Steward.generate(label="bob")
    a1 = Attestation.build(
        claims.general_claim(about="the-well", kind="observation",
                             body="the well ran clear today"),
        occasion="morning round",
    ).sign(alice, role="author")
    a2 = Attestation.build(
        claims.endorsement(target=alice.id, in_capacity="neighbor"),
        occasion="market",
        refs=[{"role": "context", "id": a1.id}],
    ).sign(bob, role="author")
    return alice, bob, a1, a2


def test_roundtrip_and_seal():
    alice, bob, a1, a2 = _fixture()
    bndl = B.make_bundle([a1, a2], sealer=alice, description="two notes")

    with tempfile.TemporaryDirectory() as d:
        path = Path(d) / "out.bundle.cbor"
        n = B.write_bundle(path, bndl)
        assert n > 0
        got = B.read_bundle(path)

    member_ids = {a.id for a in got.members()}
    assert member_ids == {a1.id, a2.id}, "members survive the round-trip"
    for a in got.members():
        ok, why = a.verified()
        assert ok, f"member failed verification after round-trip: {why}"

    ok, report = B.verify_seal(got)
    assert ok, f"sealed bundle should verify: {report}"
    assert report["sealed_by"] == alice.id
    assert report["hash_ok"] and report["members_match"]
    print("  PASS: round-trip + seal verifies, sealed_by alice")


def test_tamper_removal_detected():
    alice, bob, a1, a2 = _fixture()
    bndl = B.make_bundle([a1, a2], sealer=alice, description="two notes")
    raw = bndl.to_cbor()
    got = B.Bundle.from_cbor(raw)

    # Courier drops a1.
    got.attestations = [a for a in got.attestations if a.id != a1.id]
    ok, report = B.verify_seal(got)
    assert not ok, "removal must break the seal"
    assert a1.id in report["missing"], report
    print(f"  PASS: removal detected (missing {a1.id[:18]}...)")


def test_tamper_substitution_detected():
    alice, bob, a1, a2 = _fixture()
    carol = Steward.generate(label="carol")
    forged = Attestation.build(
        claims.general_claim(about="the-well", kind="observation",
                             body="the well was poisoned"),
        occasion="forgery",
    ).sign(carol, role="author")

    bndl = B.make_bundle([a1, a2], sealer=alice, description="two notes")
    got = B.Bundle.from_cbor(bndl.to_cbor())
    got.attestations.append(forged)  # smuggle in an unsealed member

    ok, report = B.verify_seal(got)
    assert not ok, "substitution/addition must break the seal"
    assert forged.id in report["extra"], report
    print(f"  PASS: smuggled member detected (extra {forged.id[:18]}...)")


def test_load_resolution():
    alice, bob, a1, a2 = _fixture()
    # a2 refs a1; bundle a2 only -> a1 is awaiting context until it arrives.
    partial = B.make_bundle([a2], sealer=bob)
    store = Store()
    rep = B.load_into(store, B.Bundle.from_cbor(partial.to_cbor()))
    assert a2.id in rep["loaded"]
    assert a1.id in rep["awaiting_context"], rep
    assert store.get(a2.id) is not None
    assert store.get(a1.id) is None
    print("  PASS: partial bundle loads, a1 reported awaiting context")

    # Now a follow-up bundle carries a1; the ref resolves in the same store.
    rest = B.make_bundle([a1], sealer=alice)
    rep2 = B.load_into(store, B.Bundle.from_cbor(rest.to_cbor()))
    assert a1.id in rep2["loaded"]
    ok, missing = a2.resolvable(store)
    assert ok and not missing, "a2 resolves once a1 is in the store"
    print("  PASS: follow-up bundle resolves the dangling ref")


def test_media_content_addressed():
    alice, bob, a1, a2 = _fixture()
    blob = b"a small attached photograph of the well"
    bndl = B.make_bundle([a1], media=[blob], sealer=alice)
    key = B.media_key(blob)
    assert key in bndl.media

    store = Store()
    rep = B.load_into(store, B.Bundle.from_cbor(bndl.to_cbor()))
    assert key in rep["media_ok"] and not rep["media_bad"]
    print("  PASS: good media accepted under its content-addressed key")

    # Corrupt the blob in transit; its key no longer matches its content.
    bad = B.Bundle.from_cbor(bndl.to_cbor())
    bad.media[key] = blob + b"!!"
    rep2 = B.load_into(Store(), bad)
    assert key in rep2["media_bad"] and not rep2["media_ok"]
    print("  PASS: corrupted media rejected (key/content mismatch)")


def test_unsealed_bundle_loads_but_has_no_seal():
    alice, bob, a1, a2 = _fixture()
    bare = B.make_bundle([a1, a2])  # no sealer
    assert bare.seals() == []
    store = Store()
    rep = B.load_into(store, B.Bundle.from_cbor(bare.to_cbor()))
    assert set(rep["loaded"]) == {a1.id, a2.id}
    assert rep["seal_ok"] is None, "no seal present -> seal not evaluated"
    # require_seal aborts an unsealed bundle before storing anything.
    empty = Store()
    rep2 = B.load_into(empty, bare, require_seal=True)
    assert rep2["seal_ok"] is False and len(empty) == 0
    print("  PASS: unsealed bundle loads; require_seal aborts it")


def main():
    tests = [
        test_roundtrip_and_seal,
        test_tamper_removal_detected,
        test_tamper_substitution_detected,
        test_load_resolution,
        test_media_content_addressed,
        test_unsealed_bundle_loads_but_has_no_seal,
    ]
    passed = 0
    for t in tests:
        print(f"[{t.__name__}]")
        t()
        passed += 1
    print(f"\n{passed}/{len(tests)} bundle tests passed")


if __name__ == "__main__":
    main()
