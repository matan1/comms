# Future Brief — Plural Law and Enforcement Divergence

Status: focused simulation handoff, composable with Archive Harbor and the
workstation topology.

## Purpose

Demonstrate two distinctions that prose currently states more often than the
simulator enacts:

1. Identical verified evidence can produce different legitimate judgments
   under different local policies.
2. Legitimate authority and actual host enforcement can diverge in either
   direction.

## Plural policy model

Give each Trustor or community an explicit policy head:

```js
{
  id, predecessor, community, purposes,
  issuerEligibility, evidenceClasses,
  thresholds, freshness, bodyRequirements,
  propagation, interpreterVersion
}
```

Evaluation output must name:

- viewer;
- policy id and interpreter version;
- purpose and `asOf`;
- store-view digest analogue;
- counted, ignored, challenged, and unresolved evidence;
- outcome: trusted, rejected, contested, awaiting-context.

Do not treat disagreement as error when policies or stores differ.

## Policy scenarios

- conservative port accepts only direct local interaction;
- trading port recognizes named neighbor issuers for one purpose;
- workstation requires fresh capability and enforcement receipts;
- community fork produces two policy heads;
- interpreter upgrade changes semantics while evidence remains identical;
- one policy accidentally lets trust paths become authority;
- one settlement exits and carries its law and selected evidence elsewhere.

## Authority chain

Model exact objects or sim-level analogues:

```text
resource-description
resource-request
allocation-decision
resource-grant/lease
delegated-grant
status/revocation
host-acceptance
enforcement-receipt
observed-host-state
```

Every transition names actor and role. A broker, verifier, or custodian gains no
allocation authority from handling evidence.

## Divergence cases

### Authorized, not enforced

- controller failure;
- stale policy at host;
- resource exhaustion;
- deliberate operator refusal;
- delayed revocation propagation in reverse (renewal accepted by community,
  not host).

### Enforced, not authorized

- host bypass;
- stale grant after revocation;
- delegation expands scope;
- identity/key confusion;
- tracker or broker quietly becomes decision-maker;
- compromised controller accepts a validly signed but policy-ineligible grant.

The simulator must preserve both legitimate record and observed reality. Do not
rewrite one to make the other consistent.

## Metrics

- policy-relative agreement/disagreement matrix;
- awaiting-context duration by community;
- successful cross-policy cooperation;
- accidental authority propagation;
- authorized-but-unenforced resource time;
- unauthorized enforced resource time;
- revocation and renewal latency;
- host/controller divergence detection time;
- harm prevented versus legitimate work delayed;
- exits, forks, and recognized successions.

## Acceptance

- Two viewers with identical bytes and different policies disagree with complete
  traces.
- Two viewers with one policy and different stores disagree for evidence reasons.
- At least one grant verifies cryptographically but is rejected by host policy.
- At least one host action occurs without accepted community authority and is
  recorded rather than made impossible by simulation fiat.
- A custodian transports relevant evidence without gaining grant authority.
- Competing policy heads remain contested until a viewer selects or recognizes
  succession.

