# Future Brief — Signature Attention, Clarification, and Fatigue

Status: focused experiment suitable before the full Archive Harbor build.

## Question

How should a community allocate scarce review attention when proposed acts are
cryptographically well-formed but vary in provenance, clarity, urgency, risk,
and value?

An attacker need not forge signatures. It may submit many valid, opaque,
plausible proposals until a reviewer signs carelessly or important work expires.

## Model

Each proposal has hidden research traits and participant-visible evidence:

```js
{
  risk, socialValue, urgency, complexity,
  provenanceCompleteness, purposeClarity,
  requestedRole, requestedSigner,
  contextRefs, bodyStatus, expiresDay
}
```

Reviewers have:

```js
{
  attentionPerDay, policy, expertise,
  fatigue, relationships, partialStore,
  clarificationPatience
}
```

Review actions consume different attention:

```text
scan < inspect < resolve refs < request clarification < compare < sign
```

Fatigue should affect error probability or depth of review, not become a moral
trust scalar.

## Workflow

```text
unreviewed → reviewing → approved → signed/finalized
                    ↘ awaiting-clarification → reviewing
                    ↘ deferred / declined / quarantined
```

A clarification request is itself a signed act and consumes attention from
both parties. Answers may add context, evade, conflict, or never arrive. An
answer does not imply approval.

## Actors and strategies

- careful reviewer;
- throughput-optimized reviewer;
- relationship-biased reviewer;
- policy-automated reviewer;
- honest concise proposer;
- honest but opaque specialist;
- malicious flooder;
- malicious high-urgency proposer;
- coalition that distributes proposals across distinct identities;
- clarification-abuse proposer that answers endlessly without resolving risk.

## Experiments

1. **No appraisal:** bulk-sign every matching role.
2. **Fixed review:** inspect every item equally.
3. **Risk triage:** prioritize explicit risk/purpose rules.
4. **Clarification available:** request missing provenance or reason.
5. **Quarantine:** preserve uncertain bytes outside the ordinary queue.
6. **Bounded automation:** auto-sign a closed, low-risk profile and review the
   remainder.
7. **Pending flood:** increase valid proposal arrival beyond attention supply.
8. **Distributed flood:** bypass per-proposer caps through collusion/Sybil IDs.

## Metrics

- harmful acts signed;
- beneficial acts completed and expired;
- false decline/defer;
- time to signature;
- reviewer attention consumed;
- fatigue and recovery;
- clarification requests, useful answers, evasions, and abandonment;
- queue age distribution;
- context debt at decision time;
- concentration of proposer access to reviewer attention;
- harm per signature and value per attention unit.

Separate simulator ground truth (`harmful`) from evidence visible to reviewers.
A reviewer should sometimes make a defensible decision that later proves wrong.

## Interface

Reuse the `comms-tui` conceptual desk:

- inbox and intended destination;
- exact proposed act;
- requested signer/role;
- current context and unresolved refs;
- predicted attention cost;
- appraisal state;
- clarification history;
- selected signing plan.

The simulation should allow watching the same queue through reviewer and
omniscient research perspectives.

## Acceptance

- A valid opaque flood causes measurable degradation without invalid records.
- Selected signing never sweeps an unrelated proposal.
- Clarification improves at least one outcome but imposes measurable cost.
- At least one policy suffers paralysis from excessive clarification.
- Automation is beneficial only under an explicit bounded profile.
- Results are deterministic for fixed seed and parameters.

