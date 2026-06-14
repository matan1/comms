"""Sneakernet bundles: the offline transport container of Attest 1.0.

A bundle carries a set of attestations (and optional media blobs) from one
store to another with no network in between -- USB stick, optical disc, QR
codes, an email attachment. The wire shape is fixed by the 1.0 spec
("Sneakernet bundle format"):

    {
      v:            1
      t:            "comms.bundle/1"
      attestations: [<envelope>, ...]
      media:        {<key>: <bytes>, ...}                   # optional
      manifest:     {created_at, created_by, description}   # optional
    }

A bundle is a *container, not an attestation*: by itself it authenticates none
of its membership -- a courier could drop or swap members and nothing in the
bare format would notice. Amendment A1.8 closes that with a convention, not a
new wire format: the creator seals the bundle by carrying inside it a signed
general-claim/1 whose body enumerates the member ids and binds them with
H("comms.bundle/1", canonical_cbor(manifest)). A receiver re-derives the id
list from the members actually present and checks it against the signed seal,
so removal or substitution of members is detected.

Per-member integrity is already free: every attestation id is the hash of its
own core, so altering a member either breaks one of its signatures or changes
its id -- and a changed id no longer matches the seal.

Resolution stays layered (A1.4): loading a bundle verifies each member and
reports which refs now resolve and which are still awaiting context. A
partially resolvable graph is the sneakernet norm, not an error.
"""

from __future__ import annotations

import datetime as _dt
from dataclasses import dataclass, field
from pathlib import Path

import cbor2

from .attest import Attestation
from .canonical import (CTX_BUNDLE, blake3_hash, canonical_cbor, dsh,
                        multibase_b58)
from . import claims

BUNDLE_TYPE = "comms.bundle/1"
SEAL_TAG = "comms.bundle.seal/1"


def _now() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def media_key(data: bytes) -> str:
    """Content-addressed key for a media blob: multibase blake3, shaped like an
    attestation id so the same eyeballs and tooling read it."""
    return multibase_b58(blake3_hash(data))


# ---- the A1.8 integrity seal ------------------------------------------------

def _seal_manifest(att_ids, *, created_by, description, created_at) -> dict:
    return {
        "created_at": created_at,
        "created_by": created_by,
        "description": description,
        "attestation_ids": sorted(att_ids),
    }


def seal(attestations, steward, *, description: str = "",
         created_at: str | None = None) -> Attestation:
    """Build the A1.8 integrity seal over `attestations`, signed by `steward`.

    The seal enumerates the (non-seal) member ids and binds them with a
    domain-separated bundle hash. It is meant to be carried inside the bundle;
    because it is itself a signed, content-addressed attestation, the member
    list cannot be edited without breaking the signature.
    """
    created_at = created_at or _now()
    manifest = _seal_manifest(
        [a.id for a in attestations],
        created_by=steward.id, description=description, created_at=created_at,
    )
    body = canonical_cbor({
        "t": SEAL_TAG,
        "manifest": manifest,
        "bundle_hash": dsh(CTX_BUNDLE, canonical_cbor(manifest)),
    })
    return Attestation.build(
        claims.general_claim(about="comms.bundle", kind="synthesis",
                             body=body, media_type="application/cbor"),
        occasion="bundle seal (A1.8)",
    ).sign(steward, role="author")


# ---- the container ----------------------------------------------------------

@dataclass
class Bundle:
    attestations: list = field(default_factory=list)
    media: dict = field(default_factory=dict)
    manifest: dict | None = None

    def to_cbor(self) -> bytes:
        env = {
            "v": 1,
            "t": BUNDLE_TYPE,
            "attestations": [a.to_envelope() for a in self.attestations],
        }
        if self.media:
            env["media"] = dict(self.media)
        if self.manifest is not None:
            env["manifest"] = self.manifest
        return canonical_cbor(env)

    @classmethod
    def from_cbor(cls, data: bytes) -> "Bundle":
        env = cbor2.loads(data)
        if env.get("t") != BUNDLE_TYPE:
            raise ValueError(f"not a {BUNDLE_TYPE}: t={env.get('t')!r}")
        if env.get("v") != 1:
            raise ValueError(f"unsupported bundle version: {env.get('v')!r}")
        atts = [Attestation.from_envelope(e) for e in env.get("attestations", [])]
        return cls(attestations=atts, media=dict(env.get("media", {})),
                   manifest=env.get("manifest"))

    def seals(self) -> list[tuple[Attestation, dict]]:
        """Every carried A1.8 seal, as (attestation, decoded-body) pairs."""
        out = []
        for a in self.attestations:
            if a.claim.get("t") != "general-claim/1":
                continue
            content = a.claim.get("content", {})
            if content.get("media_type") != "application/cbor":
                continue
            try:
                body = cbor2.loads(content["body"])
            except Exception:
                continue
            if isinstance(body, dict) and body.get("t") == SEAL_TAG:
                out.append((a, body))
        return out

    def members(self) -> list[Attestation]:
        """Attestations that are not themselves seals."""
        seal_ids = {a.id for a, _ in self.seals()}
        return [a for a in self.attestations if a.id not in seal_ids]


# ---- assembly ---------------------------------------------------------------

def make(attestations, *, media=None, description: str = "",
         created_by: str | None = None, created_at: str | None = None,
         sealer=None) -> Bundle:
    """Assemble a bundle.

    `media` may be a {key: bytes} map or an iterable of raw blobs (which get
    content-addressed keys). If `sealer` (a Steward) is given, an A1.8 integrity
    seal over `attestations` is generated and carried inside the bundle.
    """
    atts = list(attestations)
    created_at = created_at or _now()

    if media is None:
        media_map = {}
    elif isinstance(media, dict):
        media_map = dict(media)
    else:
        media_map = {media_key(b): b for b in media}

    if sealer is not None:
        atts = atts + [seal(attestations, sealer, description=description,
                            created_at=created_at)]

    manifest = None
    by = created_by or (sealer.id if sealer is not None else None)
    if description or by:
        manifest = {"created_at": created_at, "description": description}
        if by:
            manifest["created_by"] = by

    return Bundle(attestations=atts, media=media_map, manifest=manifest)


# ---- verification -----------------------------------------------------------

def verify_seal(bundle: Bundle) -> tuple[bool, dict]:
    """Check a bundle's A1.8 seal.

    Returns (ok, report). ok is True iff exactly one seal is present, its
    signature verifies, its declared bundle hash matches its own manifest, and
    the sealed id set equals the set of members actually present. `report`
    breaks down each check and lists `missing` (members the seal expected but
    that are absent -- a removal) and `extra` (members present but unsealed --
    an addition or substitution).
    """
    seals = bundle.seals()
    report = {"present": False, "signature_ok": False, "hash_ok": False,
              "members_match": False, "missing": [], "extra": [],
              "sealed_by": None}
    if len(seals) != 1:
        report["error"] = f"expected exactly one seal, found {len(seals)}"
        return False, report

    seal_att, body = seals[0]
    report["present"] = True
    ok, _ = seal_att.verified()
    report["signature_ok"] = ok
    report["sealed_by"] = next(iter(seal_att.signers()), None)

    manifest = body.get("manifest", {})
    report["hash_ok"] = (body.get("bundle_hash")
                         == dsh(CTX_BUNDLE, canonical_cbor(manifest)))

    sealed_ids = set(manifest.get("attestation_ids", []))
    present_ids = {a.id for a in bundle.members()}
    report["missing"] = sorted(sealed_ids - present_ids)
    report["extra"] = sorted(present_ids - sealed_ids)
    report["members_match"] = not report["missing"] and not report["extra"]

    ok_all = (report["signature_ok"] and report["hash_ok"]
              and report["members_match"])
    return ok_all, report


def load_into(store, bundle: Bundle, *, require_seal: bool = False) -> dict:
    """Verify each member and put the verified ones into `store`.

    Returns a report: `loaded` / `rejected` ids, `media_ok` / `media_bad` keys
    (a blob is accepted only if its content matches its content-addressed key),
    and -- computed against the store *after* loading -- `awaiting_context`:
    referenced ids that still do not resolve. If `require_seal` is set, an
    invalid or absent seal aborts the load before anything is stored.
    """
    report = {"seal_ok": None, "loaded": [], "rejected": [],
              "media_ok": [], "media_bad": [], "awaiting_context": []}

    if bundle.seals() or require_seal:
        report["seal_ok"], report["seal"] = verify_seal(bundle)
        if require_seal and not report["seal_ok"]:
            return report

    for key, blob in bundle.media.items():
        (report["media_ok"] if media_key(blob) == key
         else report["media_bad"]).append(key)

    for att in bundle.attestations:
        ok, why = att.verified()
        if ok:
            store.put(att)
            report["loaded"].append(att.id)
        else:
            report["rejected"].append({"id": att.id, "why": why})

    seen = set()
    for att in bundle.attestations:
        for r in att.refs:
            rid = r["id"]
            if rid in seen:
                continue
            seen.add(rid)
            if store.get(rid) is None:
                report["awaiting_context"].append(rid)
    return report


# ---- file IO ----------------------------------------------------------------

def write_bundle(path, bundle: Bundle) -> int:
    data = bundle.to_cbor()
    Path(path).write_bytes(data)
    return len(data)


def read_bundle(path) -> Bundle:
    return Bundle.from_cbor(Path(path).read_bytes())


# `make` reads well as `bundle.make(...)`; `make_bundle` reads well at top level.
make_bundle = make
