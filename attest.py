"""The Attestation envelope.

Envelope fields (short keys are the wire form):
  v : version (1)
  t : envelope type ("comms.attestation/1")
  c : claim   (map with its own "t" plus type-specific fields)
  f : frame   (issued_at, language, optional community/occasion/time_anchors)
  r : refs    (list of {role, id})
  s : signatures (list of {by, alg, signed_at, role, signature})

The canonical core (for hashing/signing) is the envelope with `s` removed.
Adding signatures never changes the attestation ID.
"""

from __future__ import annotations

import datetime as _dt
from dataclasses import dataclass, field

from .canonical import canonical_cbor, blake3_hash, attest_id
from .identity import Steward, verify_sig

ENVELOPE_TYPE = "comms.attestation/1"


def _now() -> str:
    return _dt.datetime.now(_dt.timezone.utc).isoformat()


@dataclass
class Attestation:
    claim: dict
    frame: dict
    refs: list = field(default_factory=list)
    signatures: list = field(default_factory=list)

    @classmethod
    def build(
        cls,
        claim: dict,
        *,
        language: str = "zxx",
        community: str | None = None,
        occasion: str | None = None,
        refs: list | None = None,
    ) -> "Attestation":
        frame = {"issued_at": _now(), "language": language}
        if community:
            frame["community"] = community
        if occasion:
            frame["occasion"] = occasion
        return cls(claim=claim, frame=frame, refs=refs or [])

    # --- core / id ---
    def core(self) -> dict:
        return {
            "v": 1,
            "t": ENVELOPE_TYPE,
            "c": self.claim,
            "f": self.frame,
            "r": self.refs,
        }

    @property
    def id(self) -> str:
        return attest_id(self.core())

    def _core_hash(self) -> bytes:
        return blake3_hash(canonical_cbor(self.core()))

    # --- signing ---
    def sign(self, steward: Steward, role: str = "author") -> "Attestation":
        sig = steward.sign(self._core_hash())
        self.signatures.append(
            {
                "by": steward.id,
                "alg": "ed25519",
                "signed_at": _now(),
                "role": role,
                "signature": sig,
            }
        )
        return self

    # --- serialization ---
    def to_envelope(self) -> dict:
        env = self.core()
        env["s"] = self.signatures
        return env

    def to_cbor(self) -> bytes:
        return canonical_cbor(self.to_envelope())

    @classmethod
    def from_envelope(cls, env: dict) -> "Attestation":
        return cls(
            claim=env["c"],
            frame=env["f"],
            refs=env.get("r", []),
            signatures=env.get("s", []),
        )

    @classmethod
    def from_cbor(cls, data: bytes) -> "Attestation":
        import cbor2

        return cls.from_envelope(cbor2.loads(data))

    # --- verification ---
    def signatures_valid(self) -> bool:
        h = self._core_hash()
        for s in self.signatures:
            if s.get("alg") != "ed25519":
                return False
            if not verify_sig(s["by"], h, s["signature"]):
                return False
        return True

    def signers(self) -> set[str]:
        return {s["by"] for s in self.signatures}

    def signed_by(self, steward_id: str) -> bool:
        return steward_id in self.signers()

    def verify_well_formed(self, resolver=None) -> tuple[bool, str]:
        if self.core()["v"] != 1:
            return False, "bad version"
        if self.core()["t"] != ENVELOPE_TYPE:
            return False, "bad envelope type"
        if "t" not in self.claim:
            return False, "claim missing type"
        if "issued_at" not in self.frame or "language" not in self.frame:
            return False, "frame missing required fields"
        if not self.signatures_valid():
            return False, "signature verification failed"
        if resolver is not None:
            for r in self.refs:
                if resolver.get(r["id"]) is None:
                    return False, f"unresolved ref: {r['id']}"
        return True, "ok"
