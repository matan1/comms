"""A content-addressed store of attestations, with a resolver interface and
small graph helpers for walking the attestation fabric.
"""

from __future__ import annotations

from pathlib import Path

from .attest import Attestation


class Store:
    """In-memory content-addressed store. Backs onto a directory if given."""

    def __init__(self, directory: str | Path | None = None):
        self._by_id: dict[str, Attestation] = {}
        self.directory = Path(directory) if directory else None
        if self.directory:
            self.directory.mkdir(parents=True, exist_ok=True)
            self._load_dir()

    def _load_dir(self):
        for f in self.directory.glob("*.cbor"):
            try:
                att = Attestation.from_cbor(f.read_bytes())
                self._by_id[att.id] = att
            except Exception:
                pass

    def put(self, att: Attestation) -> str:
        self._by_id[att.id] = att
        if self.directory:
            (self.directory / (att.id.split(":")[-1] + ".cbor")).write_bytes(
                att.to_cbor()
            )
        return att.id

    def get(self, att_id: str) -> Attestation | None:
        return self._by_id.get(att_id)

    def all(self) -> list[Attestation]:
        return list(self._by_id.values())

    def by_claim_type(self, claim_type: str) -> list[Attestation]:
        return [a for a in self._by_id.values() if a.claim.get("t") == claim_type]

    def referencing(self, target_id: str, role: str | None = None) -> list[Attestation]:
        """All attestations whose refs point at target_id (optionally by role)."""
        out = []
        for a in self._by_id.values():
            for r in a.refs:
                if r["id"] == target_id and (role is None or r["role"] == role):
                    out.append(a)
        return out

    def __len__(self):
        return len(self._by_id)
