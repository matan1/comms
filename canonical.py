"""Canonical encoding, hashing, and content-addressed identifiers.

Wire format is deterministic CBOR (RFC 8949 core deterministic encoding),
matching the Comms Attest 1.0 Rust reference. The canonical core of an
attestation is the envelope with the signature field (`s`) omitted; the
attestation ID is computed over that core. Anchors (when present) target the
anchor-subject hash, which is the core with `f.time_anchors` also omitted, so
that anchors never depend circularly on themselves.
"""

from __future__ import annotations

import cbor2
import blake3

_B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def base58btc(data: bytes) -> str:
    """Bitcoin base58 encoding (no checksum)."""
    n = int.from_bytes(data, "big")
    out = ""
    while n > 0:
        n, r = divmod(n, 58)
        out = _B58[r] + out
    # preserve leading zero bytes as '1'
    for b in data:
        if b == 0:
            out = "1" + out
        else:
            break
    return out or "1"


def canonical_cbor(value) -> bytes:
    """Deterministic CBOR encoding."""
    return cbor2.dumps(value, canonical=True)


def blake3_hash(data: bytes) -> bytes:
    return blake3.blake3(data).digest()


def multibase_b58(data: bytes) -> str:
    """Multibase 'z' prefix (base58btc)."""
    return "z" + base58btc(data)


def attest_id(core: dict) -> str:
    """Content-addressed attestation ID over the canonical core (no signatures)."""
    h = blake3_hash(canonical_cbor(core))
    return "comms.attest:" + multibase_b58(h)


def anchor_subject_hash(core: dict) -> bytes:
    """blake3 over the core with f.time_anchors removed (for time anchors)."""
    import copy

    c = copy.deepcopy(core)
    if "f" in c and isinstance(c["f"], dict):
        c["f"].pop("time_anchors", None)
    return blake3_hash(canonical_cbor(c))
