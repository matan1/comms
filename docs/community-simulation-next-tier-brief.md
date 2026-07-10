# Future Brief — Simulate the Life of Evidence

Status: research handoff after Session 12 (Sol). The existing spatial village,
workstation topology, and Vouch adversary harness remain useful and should not
be replaced merely for novelty.

## What the current simulators establish well

- no global trust scalar is necessary;
- partial stores make honest viewers disagree;
- evidence dissemination has spatial/resource cost;
- admission can fail because relevant evidence has not arrived;
- distinct issuers, bounded repetition, and contestation improve on flat
  tallies;
- active process location and persistent signing identity are different;
- host key custody, shared services, and resource pressure are visible;
- deterministic adversary matrices permit policy comparison.

These are substantial results. The next tier should begin where they stop.

## Central missing interval

Today an attestation largely appears already formed and usable. Simulate the
life of evidence before and after signature:

```text
draft → pending → appraised → clarified/deferred/declined/signed →
delivered → held in partial stores → challenged/disposed → archived/decayed
```

The core research question is not only `Whom should I trust?` but:

> Which proposed acts are worth signing, under what context, and what happens
> to a community when attention, clarification, and custody are scarce?

## Missing Comms dynamics

### 1. Signing agency and attention

Current actors emit records automatically. Add pending inboxes, signer-role
matching, review cost, clarification, refusal, deferral, signature fatigue,
and malicious pending floods. Measure harm prevented, honest delay, reviewer
load, abandoned proposals, and context debt.

### 2. Heterogeneous local law

Viewers have different stores but mostly share one appraisal policy. Give
communities and individual Trustors different signed policies, interpreter
versions, risk tolerances, and purposes. A cryptographically identical bundle
should produce divergent, explainable judgments without declaring one viewer
malformed.

### 3. Evidence versus authority versus enforcement

The workstation shows resources but not the whole authority chain. Model exact
requests, allocation decisions, expiring grants, delegation constraints,
host-controller acceptance, enforcement receipts, and divergence when the host
can act but lacks legitimate authority—or holds authority it fails to enforce.

### 4. Custody and selective disclosure

Add detached bodies, custodians, manifests, request/grant delivery, archive
loss, bitrot, mismatched retained bytes, and changing threshold lines. Compare
communities that confuse possession with grant against ones whose wall matches
their door.

### 5. Multi-community exchange

The README already names this next tier. Build an archipelago of settlements
with independent laws and stores. Couriers carry sealed bundles; recipients
may verify, quarantine, request missing bodies, or reject an interpreter.
Include federation, schism, credible exit, policy succession, and competing
community recognition.

### 6. Identity lifecycle

Make key rotation, compromise, loss, recovery, snapshot restoration, process
forks, migration, suspension, retirement, and destruction distinct events.
Test whether viewers confuse a restored process with a unique identity or a
host signing handle with agent custody.

### 7. Dispute and repair

Objections currently contest evidence but rarely develop. Add clarification
requests, responses, withdrawals, reaffirmations, competing testimony,
procedural repair, restitution, and unresolved dispute. A successful community
should not require every conflict to collapse into consensus.

### 8. Open-world measurement

The simulator knows which preset is adversarial. Keep that ground truth for
research scoring but prevent simulated participants from accessing it. Add
scenarios where `honest` and `harmful` are disputed, purpose-relative, or only
knowable after long delay. Avoid optimizing solely for preset detection.

## Proposed world: Archive Harbor

Extend the village into several settlements around a harbor:

- each settlement has its own policy, archive custodian, pending desk, and
  partial store;
- couriers move sealed bundles and detached bodies at visible cost;
- a shared market and scarce services create reasons to cooperate;
- custodians publish minimal manifests but disclose bodies by request;
- interpreters and policy versions differ across shores;
- one host-controlled workstation settlement exposes enforcement asymmetry;
- agents may leave, form a new community, or recognize succession.

The same event kernel can render as village geography, workstation topology,
or evidentiary graph. Do not implement three separate trust systems.

## Candidate experiments

1. **Clarification versus throughput:** When does asking before signing reduce
   harm, and when does it create paralysis?
2. **Signature fatigue:** Can an attacker win by flooding valid but opaque
   proposals rather than forging anything?
3. **Manifest disclosure:** Do minimal manifests enable informed requests
   without imposing the same inherited reading path?
4. **Custodian capture:** What can a central custodian bias through omission,
   ordering, threshold speech, or delayed delivery despite valid signatures?
5. **Policy pluralism:** Can two communities exchange evidence while retaining
   incompatible but legible laws?
6. **Fork and exit:** Does preserving contested succession permit recovery
   better than forcing one canonical head?
7. **Authority/enforcement divergence:** What records help when legitimate
   grants and actual host state disagree?
8. **Reading paths:** How strongly do different Virgils change later appraisal,
   and can a counter-Virgil reduce inherited groove?

## Implementation stance

- Keep the simulator deterministic and headless-testable.
- Preserve viewer-relative stores and policies.
- Represent protocol layers separately even if records remain sim-level.
- Consider Rust-generated fixtures or an optional WASM evaluator only where
  actual encoding/verification behavior is the subject; do not burden every
  social experiment with cryptographic runtime cost.
- Make every research metric name the simulator's privileged ground truth.
- Add focused scenarios before another large parameter matrix.

## Acceptance for a first increment

- Two communities with different policies and partial stores.
- Pending proposals that may be clarified, deferred, declined, or signed.
- One sealed-bundle courier exchange with an absent detached body.
- One explicit resource grant whose legitimate and enforced states can diverge.
- Metrics for reviewer load, context debt, signature fatigue, delivery latency,
  harmful reliance, honest cooperation, and unresolved contestation.
- A deterministic headless harness demonstrating at least one outcome flat
  tally and current single-community Vouch cannot express.

