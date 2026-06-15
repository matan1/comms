"""Claim-type constructors for an agent network.

These are the vocabulary of the experimental network. Each returns a claim dict
(a map with a "t" type tag) ready to wrap in an Attestation. Claim *types* are
open: an implementation that doesn't recognize one must still carry it.

Agent-oriented types fall into three groups:

  provenance / capability / membership   -- the agent "ceremony" vocabulary
  resource-pool / request / decision     -- coordination (resource allocation)
  endorsement / recognition / action     -- trust accumulation over time
"""

from __future__ import annotations


# ---- the general-purpose primitive ----

def general_claim(*, about: str, kind: str, body: bytes | str,
                  media_type: str = "text/plain;charset=utf-8",
                  support: list[str] | None = None) -> dict:
    """A statement about something: observation, testimony, synthesis, etc.

    kind: "observation" | "synthesis" | "prediction" | "testimony" |
          "translation" | "other"

    Per A1.6 the body is always carried as bytes on the wire regardless of
    media type, so one logical content has exactly one canonical encoding.
    """
    if isinstance(body, str):
        body = body.encode("utf-8")
    return {
        "t": "general-claim/1",
        "about": about,
        "kind": kind,
        "content": {"media_type": media_type, "body": body},
        "support": support or [],
    }


# ---- provenance & capability (the ceremony) ----

def agent_provenance(*, agent: str, kind: str, model_id: str,
                     instantiation_authority: str, code_hash: str | None = None,
                     parent: str | None = None) -> dict:
    """Who/what an agent is and where it came from.

    kind: "frontier" | "mid" | "micro" | "service" | other
    instantiation_authority: steward id of who spawned/operates it
    """
    c = {
        "t": "agent-provenance/1",
        "agent": agent,
        "kind": kind,
        "model_id": model_id,
        "instantiation_authority": instantiation_authority,
    }
    if code_hash:
        c["code_hash"] = code_hash
    if parent:
        c["parent"] = parent
    return c


def capability_challenge(*, sponsor: str, agent: str, capability: str,
                         nonce: str, params: dict) -> dict:
    """A sponsor challenges an agent to demonstrate a capability live.

    capability: "key-control" | "compute" | "storage" | ...
    params: capability-specific (e.g. {"difficulty": 12} for compute)
    """
    return {
        "t": "capability-challenge/1",
        "sponsor": sponsor,
        "agent": agent,
        "capability": capability,
        "nonce": nonce,
        "params": params,
    }


def capability_proof(*, agent: str, challenge_ref: str, response: dict) -> dict:
    """An agent's response to a challenge. Validity is checked by verifier code,
    not asserted here."""
    return {
        "t": "capability-proof/1",
        "agent": agent,
        "challenge": challenge_ref,
        "response": response,
    }


def membership_binding(*, steward: str, community: str, role: str,
                       authority: list[str], guardian: str | None = None,
                       capabilities: list[str] | None = None) -> dict:
    """An agent admitted to the community.

    role: "peer" | "ward" | "sponsor" | "allocator"
    authority: attestation ids that justify the binding (proofs, provenance)
    guardian: for "ward" agents (micro controllers) held under another steward
    """
    c = {
        "t": "membership-binding/1",
        "steward": steward,
        "community": community,
        "role": role,
        "authority": authority,
    }
    if guardian:
        c["guardian"] = guardian
    if capabilities:
        c["capabilities"] = capabilities
    return c


# ---- coordination: resource allocation ----

def resource_pool(*, pool_id: str, resource: str, total: float, period: str,
                  rule: str, allocator: str) -> dict:
    return {
        "t": "resource-pool/1",
        "pool_id": pool_id,
        "resource": resource,
        "total": total,
        "period": period,
        "rule": rule,        # human-readable name of the allocation rule
        "allocator": allocator,
    }


def allocation_request(*, agent: str, pool: str, amount: float,
                       task: str, justification: str,
                       supporting: list[str] | None = None) -> dict:
    return {
        "t": "allocation-request/1",
        "agent": agent,
        "pool": pool,
        "amount": amount,
        "task": task,
        "justification": justification,
        "supporting": supporting or [],
    }


def allocation_decision(*, pool: str, grants: list[dict], rationale: str,
                        inputs: list[str]) -> dict:
    """grants: [{"steward": id, "amount": n, "reason": str}, ...]
    inputs: attestation ids the decision consumed (audit trail)."""
    return {
        "t": "allocation-decision/1",
        "pool": pool,
        "grants": grants,
        "rationale": rationale,
        "inputs": inputs,
    }


def allocation_return(*, agent: str, pool: str, amount: float, reason: str) -> dict:
    """An agent voluntarily returns part of a grant to be redistributed (freedom)."""
    return {
        "t": "allocation-return/1",
        "agent": agent,
        "pool": pool,
        "amount": amount,
        "reason": reason,
    }


# ---- trust accumulation ----

def endorsement(*, target: str, in_capacity: str, weight: str = "primary",
                rationale: str | None = None) -> dict:
    c = {
        "t": "endorsement/1",
        "target": target,
        "in_capacity": in_capacity,
        "weight": weight,
    }
    if rationale:
        c["rationale"] = rationale
    return c


def recognition(*, agent_a: str, agent_b: str, prior: list[str],
                note: str | None = None) -> dict:
    """Two agents attest accumulated shared interaction (trust by acquaintance)."""
    c = {
        "t": "recognition/1",
        "parties": sorted([agent_a, agent_b]),
        "prior": prior,
    }
    if note:
        c["note"] = note
    return c


def action_record(*, agent: str, action: str, outcome: str,
                  detail: dict | None = None) -> dict:
    return {
        "t": "action-record/1",
        "agent": agent,
        "action": action,
        "outcome": outcome,
        "detail": detail or {},
    }


# ---- Vouch layer (viewer-relative trust policy) ----

def vouch_policy(*, community: str, name: str, purposes: list[dict],
                 anchors: list[str] | None = None,
                 description: str | None = None) -> dict:
    """A declarative Layer-4 policy.

    ``purposes`` is deliberately data, not executable code. Each entry names a
    purpose and its evidence thresholds; Vouch evaluators reject operators they
    do not understand rather than guessing.
    """
    c = {
        "t": "vouch-policy/1",
        "community": community,
        "name": name,
        "purposes": purposes,
        "anchors": anchors or [],
    }
    if description:
        c["description"] = description
    return c


def vouch_disposition(*, target: str, state: str,
                      reason: str | None = None) -> dict:
    """Activate, withdraw, or reaffirm one prior Vouch input.

    This does not revoke the target's signature. It is a new signed statement
    whose authority is evaluated relative to the issuer and selected policy.
    """
    if state not in {"active", "inactive", "reaffirmed"}:
        raise ValueError("state must be active, inactive, or reaffirmed")
    c = {"t": "vouch-disposition/1", "target": target, "state": state}
    if reason:
        c["reason"] = reason
    return c


def vouch_judgment(*, subject: str, purpose: str, policy: str, as_of: str,
                   outcome: str, store_view: str, engine: str,
                   evidence: list[str], unresolved: list[str] | None = None,
                   community: str | None = None) -> dict:
    """A portable receipt for a computed judgment, not proof it is correct."""
    if outcome not in {"trusted", "rejected", "contested", "awaiting-context"}:
        raise ValueError("unrecognized Vouch outcome")
    c = {
        "t": "vouch-judgment/1",
        "subject": subject,
        "purpose": purpose,
        "policy": policy,
        "as_of": as_of,
        "outcome": outcome,
        "store_view": store_view,
        "engine": engine,
        "evidence": evidence,
        "unresolved": unresolved or [],
    }
    if community:
        c["community"] = community
    return c
