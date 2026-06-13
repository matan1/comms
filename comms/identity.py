"""Steward: a cryptographic identity in Comms.

A single-key steward is an Ed25519 keypair. Its ID is the multibase-encoded
public key. Multi-key (community / threshold) stewards are identifiable in this
build but not yet verifiable; that awaits the Steward-layer keyset descriptor.
"""

from __future__ import annotations

import json
from pathlib import Path

import nacl.signing
import nacl.encoding

from .canonical import multibase_b58, base58btc, _B58


def _b58decode(s: str) -> bytes:
    n = 0
    for ch in s:
        n = n * 58 + _B58.index(ch)
    full = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    pad = 0
    for ch in s:
        if ch == "1":
            pad += 1
        else:
            break
    return b"\x00" * pad + full


class Steward:
    """A single-key steward identity."""

    def __init__(self, signing_key: nacl.signing.SigningKey, label: str = ""):
        self._sk = signing_key
        self.vk = signing_key.verify_key
        self.label = label  # local nickname only; never part of identity

    @classmethod
    def generate(cls, label: str = "") -> "Steward":
        return cls(nacl.signing.SigningKey.generate(), label)

    @property
    def id(self) -> str:
        return "comms.steward:" + multibase_b58(bytes(self.vk))

    @property
    def pubkey(self) -> bytes:
        return bytes(self.vk)

    def sign(self, message: bytes) -> bytes:
        return self._sk.sign(message).signature

    def save(self, path: str | Path) -> None:
        p = Path(path)
        p.write_text(
            json.dumps(
                {"seed_b58": base58btc(bytes(self._sk)), "label": self.label}
            )
        )
        p.chmod(0o600)

    @classmethod
    def load(cls, path: str | Path) -> "Steward":
        data = json.loads(Path(path).read_text())
        sk = nacl.signing.SigningKey(_b58decode(data["seed_b58"]))
        return cls(sk, data.get("label", ""))


def verify_sig(steward_id: str, message: bytes, signature: bytes) -> bool:
    """Verify a signature against a steward ID's embedded public key."""
    if not steward_id.startswith("comms.steward:z"):
        return False
    pub = _b58decode(steward_id[len("comms.steward:z"):])
    try:
        nacl.signing.VerifyKey(pub).verify(message, signature)
        return True
    except Exception:
        return False
