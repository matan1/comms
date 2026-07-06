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

Signing follows Amendment A1.3: each signature is plain Ed25519 over the
canonical CBOR of a payload {t: "comms.sig/1", core, by, alg, role,
signed_at}, so the signer's metadata is bound to the core — a witness
signature cannot be re-presented as a sponsor signature.

Validation is layered per A1.4: `structurally_valid()` (the document alone),
`verified()` (every signature checks out), `resolvable(store)` (refs resolve
in a context). Resolution failure is "awaiting context", never malformedness.
Trust is not a protocol property and has no method here.
"""

from __future__ import annotations

import datetime as _dt
import re as _re
from dataclasses import dataclass, field

from .canonical import canonical_cbor, core_hash, attest_id
from .identity import Steward, verify_sig

ENVELOPE_TYPE = "comms.attestation/1"
SIG_ALG = "ed25519"

# A1.6: RFC 3339 UTC, Z suffix, no fractional seconds — one canonical encoding
# per instant.
_TIMESTAMP_RE = _re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")


def _now() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sig_payload(core: dict, *, by: str, alg: str, role: str,
                signed_at: str) -> bytes:
    """The canonical CBOR signature payload of A1.3."""
    return canonical_cbor({
        "t": "comms.sig/1",
        "core": core_hash(core),
        "by": by,
        "alg": alg,
        "role": role,
        "signed_at": signed_at,
    })


def _content_well_formed(content) -> tuple[bool, str]:
    """A2.1 layer-1 shape of a general-claim/1 content map: exactly one of
    body/body_b3; detached requires body_len; embedded must not carry it."""
    if not isinstance(content, dict):
        return False, "general-claim carries no content map"
    has_body = "body" in content
    has_b3 = "body_b3" in content
    if has_body and has_b3:
        return False, "content carries both body and body_b3 (A2.1)"
    if not has_body and not has_b3:
        return False, "content carries neither body nor body_b3 (A2.1)"
    if has_b3:
        b3, length = content["body_b3"], content.get("body_len")
        if not isinstance(b3, bytes) or len(b3) != 32:
            return False, "body_b3 is not 32 bytes (A2.1)"
        if not isinstance(length, int) or isinstance(length, bool) or length < 0:
            return False, "detached content requires body_len (A2.1)"
    else:
        if not isinstance(content["body"], bytes):
            return False, "body is not a byte string (A1.6)"
        if "body_len" in content:
            return False, "embedded content must not carry body_len (A2.1)"
    return True, "ok"


def body_status(content: dict, data: bytes | None = None) -> str:
    """A2.2 body status — a judgment separate from attestation validity:

    - "verified":   bytes at hand honor the commitment (an embedded body is
                    trivially verified — the bytes are the commitment)
    - "absent":     detached and no bytes at hand; the normal state for
                    anything host-gated, never an error
    - "mismatched": bytes at hand under the body's name, but hash or length
                    differs; per the preservation stance such bytes are
                    retained and reported, never deleted or silently repaired

    Callers must not fold "absent"/"mismatched" into signature failure.
    """
    from .canonical import blake3_hash

    if "body" in content:
        return "verified"
    if data is None:
        return "absent"
    if blake3_hash(data) == content.get("body_b3") \
            and len(data) == content.get("body_len"):
        return "verified"
    return "mismatched"


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
        return core_hash(self.core())

    # --- signing ---
    def sign(self, steward: Steward, role: str = "author",
             signed_at: str | None = None) -> "Attestation":
        signed_at = signed_at or _now()
        payload = sig_payload(self.core(), by=steward.id, alg=SIG_ALG,
                              role=role, signed_at=signed_at)
        self.signatures.append(
            {
                "by": steward.id,
                "alg": SIG_ALG,
                "signed_at": signed_at,
                "role": role,
                "signature": steward.sign(payload),
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

    # --- validation layers (A1.4) ---
    def structurally_valid(self) -> tuple[bool, str]:
        """Layer 1: a property of the document alone — required fields,
        identifiers parse, timestamps canonical, no duplicate signatures."""
        core = self.core()
        if core["v"] != 1:
            return False, "bad version"
        if core["t"] != ENVELOPE_TYPE:
            return False, "bad envelope type"
        if not isinstance(self.claim, dict) or "t" not in self.claim:
            return False, "claim missing type"
        if self.claim.get("t") == "general-claim/1":
            ok, why = _content_well_formed(self.claim.get("content"))
            if not ok:
                return False, why
        if "issued_at" not in self.frame or "language" not in self.frame:
            return False, "frame missing required fields"
        if not _TIMESTAMP_RE.match(self.frame["issued_at"]):
            return False, "frame issued_at not canonical RFC 3339 Z form"
        for r in self.refs:
            if "role" not in r or "id" not in r:
                return False, "ref missing role or id"
            if not str(r["id"]).startswith("comms.attest:z"):
                return False, f"ref id does not parse: {r['id']}"
        seen = set()
        for s in self.signatures:
            for k in ("by", "alg", "signed_at", "role", "signature"):
                if k not in s:
                    return False, f"signature missing field: {k}"
            if s["alg"] != SIG_ALG:
                return False, f"unrecognized signature alg: {s['alg']}"
            if not _TIMESTAMP_RE.match(s["signed_at"]):
                return False, "signature signed_at not canonical RFC 3339 Z form"
            wire = canonical_cbor(s)
            if wire in seen:
                return False, "duplicate signature object"
            seen.add(wire)
        return True, "ok"

    def signatures_valid(self) -> bool:
        """Every signature verifies under A1.3 (payload reconstructed from the
        signature object's own fields plus the locally computed core hash)."""
        core = self.core()
        for s in self.signatures:
            if s.get("alg") != SIG_ALG:
                return False
            payload = sig_payload(core, by=s["by"], alg=s["alg"],
                                  role=s["role"], signed_at=s["signed_at"])
            if not verify_sig(s["by"], payload, s["signature"]):
                return False
        return True

    def verified(self) -> tuple[bool, str]:
        """Layer 2: structurally valid and every signature verifies."""
        ok, why = self.structurally_valid()
        if not ok:
            return False, why
        if not self.signatures_valid():
            return False, "signature verification failed"
        return True, "ok"

    def resolvable(self, resolver) -> tuple[bool, list[str]]:
        """Layer 3: every ref resolves in the given store/context. A failure
        here means "awaiting context", not malformedness."""
        missing = [r["id"] for r in self.refs if resolver.get(r["id"]) is None]
        return not missing, missing

    def verify_well_formed(self, resolver=None) -> tuple[bool, str]:
        """Compatibility wrapper: layers 1-2, plus layer 3 if a resolver is
        given. Prefer the layered methods, which keep "unresolved" distinct
        from "malformed"."""
        ok, why = self.verified()
        if not ok:
            return False, why
        if resolver is not None:
            ok, missing = self.resolvable(resolver)
            if not ok:
                return False, f"unresolved ref: {missing[0]}"
        return True, "ok"

    def signers(self) -> set[str]:
        return {s["by"] for s in self.signatures}

    def signed_by(self, steward_id: str) -> bool:
        return steward_id in self.signers()
