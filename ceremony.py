"""Agent ceremonies.

The human coming-into-community ceremony grounds the key/person binding in
physical, witnessed recognition. Agents have no bodies to gather, but they have
something humans lack: you can challenge an agent to *demonstrate* a capability
live, right now, and verify the demonstration. So agent trust grounds in
verifiable demonstrated behavior rather than embodied co-presence.

Four rites, mapping onto the human structure but adapted:

  capability        -- challenge / response / witnessed verification.
                       The agent analog of "we watched them actually do it."
  provenance        -- the agent presents its lineage; the instantiation
                       authority co-signs. "We know where this one came from."
  guardianship      -- a micro agent that can't bear rich crypto is brought in
                       under a guardian who holds recovery and bounds authority.
                       The agent analog of bringing in a tool or a ward.
  recognition       -- two agents attest accumulated interaction. Trust by
                       acquaintance, built over repeated dealings.

Capability challenges are real and checkable here:

  key-control : sign the nonce. Proves control of the key. (any agent)
  compute     : find x such that blake3(nonce||x) has `difficulty` leading zero
                bits. Tunable cost lets a frontier agent prove more than a micro
                one -- the proof's difficulty becomes evidence for allocation.
  storage     : produce a blake3 merkle root over a claimed dataset of `n` leaves
                (here a stand-in; real storage proofs come later).
"""

from __future__ import annotations

import os
import secrets

import blake3

from .attest import Attestation
from .identity import Steward, verify_sig
from .canonical import canonical_cbor
from . import claims as C


def new_nonce() -> str:
    return secrets.token_hex(16)


# ---------- capability verification ----------

def _leading_zero_bits(b: bytes) -> int:
    bits = 0
    for byte in b:
        if byte == 0:
            bits += 8
            continue
        for i in range(7, -1, -1):
            if byte & (1 << i):
                return bits
            bits += 1
        break
    return bits


def solve_compute(nonce: str, difficulty: int, budget: int = 5_000_000) -> dict | None:
    """Find x so blake3(nonce||x) has >= difficulty leading zero bits."""
    target = nonce.encode()
    for i in range(budget):
        x = i.to_bytes(8, "big")
        h = blake3.blake3(target + x).digest()
        if _leading_zero_bits(h) >= difficulty:
            return {"x": x.hex(), "digest": h.hex()}
    return None


def verify_capability(challenge: dict, proof_response: dict,
                      proof_signer: str) -> tuple[bool, str]:
    """Verify a capability proof against its challenge."""
    cap = challenge["capability"]
    nonce = challenge["nonce"]

    if cap == "key-control":
        sig = bytes.fromhex(proof_response["signature"])
        ok = verify_sig(proof_signer, nonce.encode(), sig)
        return ok, "key-control verified" if ok else "bad signature"

    if cap == "compute":
        difficulty = challenge["params"]["difficulty"]
        x = bytes.fromhex(proof_response["x"])
        h = blake3.blake3(nonce.encode() + x).digest()
        if h.hex() != proof_response.get("digest"):
            return False, "digest mismatch"
        bits = _leading_zero_bits(h)
        if bits < difficulty:
            return False, f"insufficient work: {bits} < {difficulty}"
        return True, f"compute verified ({bits} leading zero bits)"

    if cap == "storage":
        claimed_root = proof_response.get("root")
        leaves = proof_response.get("leaves", [])
        root = _merkle_root([bytes.fromhex(l) for l in leaves])
        ok = root.hex() == claimed_root
        return ok, "storage root verified" if ok else "root mismatch"

    return False, f"unknown capability: {cap}"


def _merkle_root(leaves: list[bytes]) -> bytes:
    if not leaves:
        return b"\x00" * 32
    layer = [blake3.blake3(l).digest() for l in leaves]
    while len(layer) > 1:
        nxt = []
        for i in range(0, len(layer), 2):
            a = layer[i]
            b = layer[i + 1] if i + 1 < len(layer) else layer[i]
            nxt.append(blake3.blake3(a + b).digest())
        layer = nxt
    return layer[0]


# ---------- proof construction (agent side) ----------

def make_proof_response(agent: Steward, challenge: dict) -> dict | None:
    cap = challenge["capability"]
    nonce = challenge["nonce"]
    if cap == "key-control":
        return {"signature": agent.sign(nonce.encode()).hex()}
    if cap == "compute":
        return solve_compute(nonce, challenge["params"]["difficulty"])
    if cap == "storage":
        n = challenge["params"].get("leaves", 4)
        leaves = [os.urandom(16) for _ in range(n)]
        return {
            "leaves": [l.hex() for l in leaves],
            "root": _merkle_root(leaves).hex(),
        }
    return None


# ---------- the rites ----------

class Network:
    """Glue that runs ceremonies and records every attestation to a store."""

    def __init__(self, store, community: Steward):
        self.store = store
        self.community = community  # the steward that holds the network's rule

    def _record(self, att: Attestation) -> str:
        return self.store.put(att)

    # capability rite -------------------------------------------------------
    def capability_rite(self, sponsor: Steward, agent: Steward,
                        capability: str, params: dict,
                        witnesses: list[Steward] | None = None) -> dict:
        """Run a full capability ceremony. Returns the resulting attestation ids."""
        nonce = new_nonce()
        chal = Attestation.build(
            C.capability_challenge(
                sponsor=sponsor.id, agent=agent.id,
                capability=capability, nonce=nonce, params=params,
            ),
            community=self.community.id,
            occasion=f"capability rite: {capability}",
        ).sign(sponsor, role="sponsor")
        chal_id = self._record(chal)

        response = make_proof_response(agent, chal.claim)
        if response is None:
            return {"ok": False, "stage": "response", "challenge": chal_id}

        proof = Attestation.build(
            C.capability_proof(agent=agent.id, challenge_ref=chal_id,
                               response=response),
            community=self.community.id,
            refs=[{"role": "responds-to", "id": chal_id}],
        ).sign(agent, role="author")
        proof_id = self._record(proof)

        ok, detail = verify_capability(chal.claim, response, agent.id)
        if not ok:
            return {"ok": False, "stage": "verify", "detail": detail,
                    "challenge": chal_id, "proof": proof_id}

        # sponsor + witnesses endorse the proof
        endorsers = [sponsor] + (witnesses or [])
        end = Attestation.build(
            C.endorsement(target=proof_id,
                          in_capacity=f"capability:{capability}",
                          rationale=detail),
            community=self.community.id,
            refs=[{"role": "responds-to", "id": proof_id}],
        )
        for e in endorsers:
            end.sign(e, role="witness")
        end_id = self._record(end)

        return {"ok": True, "detail": detail, "challenge": chal_id,
                "proof": proof_id, "endorsement": end_id}

    # provenance rite -------------------------------------------------------
    def provenance_rite(self, agent: Steward, authority: Steward, *,
                        kind: str, model_id: str, code_hash: str | None = None,
                        parent: str | None = None) -> str:
        att = Attestation.build(
            C.agent_provenance(
                agent=agent.id, kind=kind, model_id=model_id,
                instantiation_authority=authority.id,
                code_hash=code_hash, parent=parent,
            ),
            community=self.community.id,
            occasion="provenance rite",
        )
        att.sign(agent, role="subject")
        att.sign(authority, role="author")  # authority co-signs lineage
        return self._record(att)

    # admission (binding) ---------------------------------------------------
    def admit(self, sponsor: Steward, agent: Steward, *, role: str,
              authority: list[str], guardian: Steward | None = None,
              capabilities: list[str] | None = None) -> str:
        att = Attestation.build(
            C.membership_binding(
                steward=agent.id, community=self.community.id, role=role,
                authority=authority,
                guardian=guardian.id if guardian else None,
                capabilities=capabilities,
            ),
            community=self.community.id,
            occasion="admission",
            refs=[{"role": "context", "id": a} for a in authority],
        )
        att.sign(sponsor, role="sponsor")
        att.sign(self.community, role="community")
        return self._record(att)

    # guardianship rite (micro agents) -------------------------------------
    def guardianship_rite(self, guardian: Steward, ward: Steward, *,
                         model_id: str, capabilities: list[str]) -> dict:
        """A micro agent brought in under a guardian. The ward proves only
        key-control; the guardian vouches and holds responsibility."""
        prov = self.provenance_rite(
            ward, guardian, kind="micro", model_id=model_id, parent=guardian.id
        )
        cap = self.capability_rite(guardian, ward, "key-control", {})
        binding = self.admit(
            guardian, ward, role="ward",
            authority=[prov, cap.get("endorsement", cap.get("proof"))],
            guardian=guardian, capabilities=capabilities,
        )
        return {"provenance": prov, "capability": cap, "binding": binding}

    # recognition rite ------------------------------------------------------
    def recognition_rite(self, a: Steward, b: Steward, prior: list[str],
                        note: str | None = None) -> str:
        att = Attestation.build(
            C.recognition(agent_a=a.id, agent_b=b.id, prior=prior, note=note),
            community=self.community.id,
            occasion="mutual recognition",
            refs=[{"role": "context", "id": p} for p in prior],
        )
        att.sign(a, role="author")
        att.sign(b, role="author")
        return self._record(att)
