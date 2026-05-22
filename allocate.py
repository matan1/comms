"""The convivial allocator.

This is the project's philosophy expressed as decision code rather than as a
value statement. The four load-bearing values appear as mechanics:

  community     -- a request's weight is lifted by peer endorsements, not just
                   self-assertion. You cannot vouch for yourself.
  diversity     -- a per-agent seed floor: every bound member gets a guaranteed
                   minimum before anything is allocated by merit. "Anyone can
                   plant a seed once it is in hand."
  understanding -- the decision attests every input it consumed; the whole
                   allocation is auditable from the attestation graph.
  freedom       -- agents may decline or return a grant; returns are
                   redistributed by the same rule.

Anti-concentration is NOT a separate equity correction. It falls out of the
rule: the seed floor guarantees the small, and a per-agent cap (a fraction of
the merit pool) prevents any single agent sweeping it. Distribution is a
consequence of the substrate, as intended.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class AllocatorRule:
    seed_fraction: float = 0.20   # share of the pool reserved as equal seed
    per_agent_cap_fraction: float = 0.40  # max share of the *merit* pool to one agent
    require_membership: bool = True


def allocate(*, total: float, members: dict, requests: list, endorsements: dict,
             capability_scores: dict, rule: AllocatorRule) -> dict:
    """Compute an allocation.

    members            : {steward_id: role}
    requests           : [{"agent","amount","task","justification","id"}, ...]
    endorsements       : {request_id: [endorser_steward_id, ...]}
    capability_scores  : {steward_id: float}  (e.g. demonstrated compute bits)
    Returns {"grants": [...], "rationale": str, "inputs": [...], "trace": {...}}.
    """
    inputs = []
    eligible = []
    for r in requests:
        inputs.append(r["id"])
        if rule.require_membership and r["agent"] not in members:
            continue
        eligible.append(r)

    member_ids = [m for m in members]
    n = len(member_ids)
    if n == 0:
        return {"grants": [], "rationale": "no members", "inputs": inputs, "trace": {}}

    # 1. seed floor: equal share to every member, regardless of request
    seed_pool = total * rule.seed_fraction
    seed_each = seed_pool / n
    grants = {m: seed_each for m in member_ids}

    # 2. merit pool distributed by need x capability x community-vouch
    merit_pool = total - seed_pool
    scores = {}
    for r in eligible:
        agent = r["agent"]
        # community: count endorsements that are NOT self-endorsements
        vouches = [e for e in endorsements.get(r["id"], []) if e != agent]
        community_factor = 1.0 + 0.5 * len(vouches)
        capability_factor = 1.0 + capability_scores.get(agent, 0.0)
        # need: how much, above seed, the agent asked for (bounded)
        need = max(0.0, r["amount"] - seed_each)
        scores[agent] = scores.get(agent, 0.0) + need * community_factor * capability_factor

    total_score = sum(scores.values())
    per_agent_cap = merit_pool * rule.per_agent_cap_fraction
    trace = {"seed_each": seed_each, "merit_pool": merit_pool, "scores": dict(scores)}

    if total_score > 0:
        # provisional proportional split, then cap, then redistribute overflow
        raw = {a: merit_pool * (s / total_score) for a, s in scores.items()}
        capped = {a: min(v, per_agent_cap) for a, v in raw.items()}
        overflow = merit_pool - sum(capped.values())
        # redistribute overflow to uncapped agents by score, one pass
        uncapped = {a: scores[a] for a in capped if capped[a] < per_agent_cap}
        us = sum(uncapped.values())
        if us > 0 and overflow > 1e-9:
            for a in uncapped:
                add = overflow * (uncapped[a] / us)
                capped[a] = min(per_agent_cap, capped[a] + add)
        for a, v in capped.items():
            grants[a] += v
        trace["raw"] = raw
        trace["capped"] = capped
        trace["overflow_redistributed"] = overflow

    grant_list = [
        {"steward": m, "amount": round(grants[m], 4),
         "reason": _reason(m, members, scores, seed_each)}
        for m in member_ids
    ]
    rationale = (
        f"seed floor {rule.seed_fraction:.0%} split equally among {n} members "
        f"({seed_each:.2f} each); remaining {merit_pool:.2f} by "
        f"need x capability x peer-vouch, capped at "
        f"{rule.per_agent_cap_fraction:.0%} of merit pool per agent."
    )
    return {"grants": grant_list, "rationale": rationale, "inputs": inputs,
            "trace": trace}


def _reason(m, members, scores, seed_each):
    role = members[m]
    if m not in scores:
        return f"seed floor only ({seed_each:.2f}); no eligible request"
    return f"seed + merit (role={role}, score={scores[m]:.2f})"
