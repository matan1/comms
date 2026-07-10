# Future Brief — Build Archive Harbor

Status: staged implementation handoff. Depends on
`community-simulation-gap-analysis.md` and
`community-simulation-next-tier-brief.md`.

## Outcome

Extend the spatial simulator into two or more independent communities around a
harbor. Each has its own signed-law analogue, archive custodian, pending desk,
partial stores, and resource constraints. Couriers transport sealed bundles
and detached bodies. Communities may cooperate, disagree, federate, fork, or
exit without acquiring one global trust system.

## Preserve the existing kernel

Do not fork separate village, workstation, and harbor trust engines. Extend the
current viewer-relative evidence kernel and render it through different maps.

Existing entry points remain useful:

```text
buildWorld(p)
seedState(p)
advanceDay(p)
addAttestation(fields, knowers)
learn(viewer, attestationIndex)
perceivedTrust / perceivedVouch
gossipByProximity
```

## State additions

Candidate sim-level state:

```js
state.communities = [{
  id, label, center, members, policyHead,
  archive: {
    custodian, store, bodies, pending, manifest,
    deliveries, drift
  },
  resources, neighbors
}];

state.proposals = [{
  id, core, body, author, requestedSigners,
  status, clarificationIds, signatures,
  createdDay, expiresDay
}];

state.bundles = [{
  id, sender, recipient, memberIds, bodyHashes,
  sealOk, location, courier, arrivalDay
}];

state.policies = [{
  id, community, predecessor, purposes,
  thresholds, bodyRules, issuerRules
}];
```

Keep simulator indexes distinct from protocol IDs. Where actual fixtures are
used, store their real IDs as additional fields.

## Stage 1 — Community plurality

- Replace implicit one-community membership with `communityId`.
- Give each community a separately selected appraisal policy.
- Scope ceremonies, sponsors, membership, and priors to a community.
- Let a viewer hold evidence from several communities without treating foreign
  membership as local authority.
- Render community boundaries and the selected viewer's recognized neighbors.

Acceptance: two viewers with identical stores but different policies produce
different, explained outcomes.

## Stage 2 — Harbor transport

- Add docks, couriers, travel/message cost, departures, arrivals, and loss or
  delay events.
- A courier carries a bundle, not ambient gossip.
- Receipt verifies member set and available bodies independently.
- Detached bodies may be absent even when their attestations arrive.
- Recipients may intake, quarantine, request context/body, or decline.

Acceptance: one sealed bundle arrives intact but cannot support a decision
because a relevant body or interpreter is absent.

## Stage 3 — Archive wall

Each custodian maintains:

- content-addressed attestation/body custody;
- a deterministic minimal manifest;
- request, grant, defer, and decline events;
- delivery records;
- audit drift and preserved mismatches;
- one changing threshold line outside the custody snapshot.

Do not grant bodies automatically because two communities are allied. Model
possession, grant, custody, and reliance separately.

Acceptance: compare a community whose archive wall is merely policy against
one where grant performs delivery. Measure unrecorded access and failed body
resolution.

## Stage 4 — Pending desks

Add proposals requiring signatures before they become external acts. Reviewers
have bounded attention and may approve, clarify, defer, decline, quarantine,
or sign. Signed clarification never endorses the target proposal.

Acceptance: a pending flood increases reviewer load without requiring invalid
cryptography; clarification prevents some harm while delaying honest work.

## Stage 5 — Authority and enforcement

At least one workstation settlement has host-enforced resources:

```text
resource request
allocation decision
scoped grant/lease
optional delegation
host acceptance
actual enforced state
enforcement receipt
```

Permit both divergence directions: authorized but not enforced; enforced but
not legitimately authorized.

## Stage 6 — Fork, federation, and exit

- Signed policy succession can produce competing heads.
- A settlement may recognize one head, await context, or preserve contestation.
- Members may leave and found a new community carrying selected bundles.
- Federation is explicit recognition for named purposes, not trust transitivity.
- Couriers and custodians do not become authorities merely by connecting shores.

## Interface

Add projections rather than omniscient answers:

- geographic harbor and courier movement;
- evidence graph and body availability;
- selected viewer's recognized laws and judgments;
- custodian wall/intake/delivery view;
- pending attention queue;
- legitimate authority versus observed host state.

Research ground truth may color an omniscient laboratory view but must disappear
from participant views.

## First deterministic harness

Run 20–50 seeds over a short fixed horizon with:

- two communities and two policies;
- one courier route;
- one missing detached body;
- honest and opaque pending proposals;
- bounded reviewer attention;
- one host enforcement divergence;
- optional clarification.

Report:

- cross-community cooperation completed;
- harmful reliance;
- awaiting-context and contested duration;
- reviewer load and signature latency;
- clarification count and value;
- bundle/body delivery latency;
- unauthorized enforcement and unenforced legitimate grants;
- exit/fork survival.

Do not define success as maximum agreement. A plural community system may be
healthy precisely because disagreements remain legible.

