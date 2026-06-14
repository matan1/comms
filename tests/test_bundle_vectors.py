"""Golden-vector conformance for the sneakernet bundle format + A1.8 seal
(data/attest-1.0-bundle-vectors.json).

The vectors are derived independently in scripts/gen_vectors.py (cbor2/blake3/
nacl only, no comms import). This test proves the shipping comms.bundle
reproduces them byte-for-byte and rejects both negative bundles -- the same
bar A1.9 sets for the attestation vectors, now extended to the container that
will carry everything over sneakernet. When the Rust port arrives it must pass
this same file.

Runnable under pytest or directly:  python tests/test_bundle_vectors.py
"""

from __future__ import annotations

import json
from pathlib import Path

from comms import bundle as B
from comms.canonical import CTX_BUNDLE, canonical_cbor, dsh

VECTORS = json.loads(
    (Path(__file__).resolve().parent.parent / "data"
     / "attest-1.0-bundle-vectors.json").read_text()
)


def test_manifest_canonical_cbor_and_bundle_hash():
    """comms canonical CBOR of the manifest matches the frozen bytes, and the
    domain-separated bundle hash over them matches H('comms.bundle/1', .)."""
    seal = VECTORS["seal"]
    manifest_cbor = canonical_cbor(seal["manifest"])
    assert manifest_cbor.hex() == seal["manifest_canonical_cbor_hex"], \
        "manifest canonical CBOR diverged from the vector"
    assert dsh(CTX_BUNDLE, manifest_cbor).hex() == seal["bundle_hash_hex"], \
        "bundle hash diverged from the vector"
    print("  PASS: manifest CBOR + bundle hash reproduce the vector")


def test_positive_bundle_verifies_and_ids_match():
    spec = VECTORS["bundle"]
    bndl = B.Bundle.from_cbor(bytes.fromhex(spec["canonical_cbor_hex"]))

    ok, report = B.verify_seal(bndl)
    assert ok, f"sealed vector bundle must verify: {report}"
    assert report["sealed_by"] == spec["expect"]["sealed_by"]
    assert report["missing"] == [] and report["extra"] == []

    member_ids = sorted(a.id for a in bndl.members())
    assert member_ids == sorted(spec["member_ids"]), \
        "member ids reconstructed from the bundle diverged"

    seals = bndl.seals()
    assert len(seals) == 1
    seal_att, _ = seals[0]
    # Re-canonicalizing the seal core must reproduce the frozen id, which only
    # holds if comms' CBOR + hashing match the independent generator's.
    assert seal_att.id == spec["seal_id"], "seal id diverged (encoding mismatch)"
    print("  PASS: positive bundle verifies; member + seal ids match the vector")


def test_media_key_rule():
    mk = VECTORS["media_key_example"]
    assert B.media_key(mk["body_utf8"].encode()) == mk["media_key"], \
        "media key (raw blake3, multibase) diverged from the vector"
    print("  PASS: media key rule reproduces the vector")


def test_negative_dropped_member():
    neg = VECTORS["negative_vectors"][0]
    bndl = B.Bundle.from_cbor(bytes.fromhex(neg["canonical_cbor_hex"]))
    ok, report = B.verify_seal(bndl)
    assert not ok, "dropped-member bundle must fail the seal"
    assert report["missing"] == neg["expect"]["missing"], report
    assert report["extra"] == neg["expect"]["extra"], report
    print(f"  PASS: dropped member rejected (missing {report['missing'][0][:18]}...)")


def test_negative_smuggled_member():
    neg = VECTORS["negative_vectors"][1]
    bndl = B.Bundle.from_cbor(bytes.fromhex(neg["canonical_cbor_hex"]))
    ok, report = B.verify_seal(bndl)
    assert not ok, "smuggled-member bundle must fail the seal"
    assert report["missing"] == neg["expect"]["missing"], report
    assert report["extra"] == neg["expect"]["extra"], report
    assert neg["forged_id"] in report["extra"]
    print(f"  PASS: smuggled member rejected (extra {report['extra'][0][:18]}...)")


def main():
    tests = [
        test_manifest_canonical_cbor_and_bundle_hash,
        test_positive_bundle_verifies_and_ids_match,
        test_media_key_rule,
        test_negative_dropped_member,
        test_negative_smuggled_member,
    ]
    for t in tests:
        print(f"[{t.__name__}]")
        t()
    print(f"\n{len(tests)}/{len(tests)} bundle-vector conformance checks passed")


if __name__ == "__main__":
    main()
