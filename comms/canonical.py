"""Canonical encoding, hashing, and content-addressed identifiers.

Wire format is deterministic CBOR (RFC 8949 core deterministic encoding),
matching the Comms Attest 1.0 Rust reference. Per Amendment A1.1 every blake3
hash in the protocol is domain separated: H(ctx, D) = blake3(uint8(len(ctx))
|| ctx || D). The canonical core of an attestation is the envelope with the
signature field (`s`) omitted; the attestation ID is computed over that core
under the `comms.attest.core/1` context. Anchors (when present) target the
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


# Hash contexts defined by Attest 1.0 as amended by A1.1.
CTX_CORE = b"comms.attest.core/1"
CTX_KEYSET = b"comms.keyset/1"
CTX_BUNDLE = b"comms.bundle/1"


def canonical_cbor(value) -> bytes:
    """Deterministic CBOR encoding."""
    return cbor2.dumps(value, canonical=True)


def dsh(ctx: bytes, data: bytes) -> bytes:
    """Domain-separated hash: blake3(uint8(len(ctx)) || ctx || data)  (A1.1)."""
    if not 0 < len(ctx) < 256:
        raise ValueError("hash context must be 1..255 bytes")
    return blake3.blake3(bytes([len(ctx)]) + ctx + data).digest()


def blake3_hash(data: bytes) -> bytes:
    """Raw blake3. Protocol hashing must use dsh(); this exists for non-protocol
    uses (e.g. content body digests named by media type)."""
    return blake3.blake3(data).digest()


def multibase_b58(data: bytes) -> str:
    """Multibase 'z' prefix (base58btc)."""
    return "z" + base58btc(data)


def core_hash(core: dict) -> bytes:
    """Domain-separated hash of the canonical core (A1.2)."""
    return dsh(CTX_CORE, canonical_cbor(core))


def attest_id(core: dict) -> str:
    """Content-addressed attestation ID over the canonical core (no signatures)."""
    return "comms.attest:" + multibase_b58(core_hash(core))


def anchor_subject_hash(core: dict) -> bytes:
    """Hash of the core with f.time_anchors removed (for time anchors).
    Uses the core context: a core without time_anchors hashes identically
    either way, so anchors target exactly what the ID would have been."""
    import copy

    c = copy.deepcopy(core)
    if "f" in c and isinstance(c["f"], dict):
        c["f"].pop("time_anchors", None)
    return dsh(CTX_CORE, canonical_cbor(c))
