# Vouch 1.0 Requirements

Status: candidate requirements for Layer 4. They do not amend Attest 1.0 or
Steward 1.0 and do not make the reference policy authoritative.

## Purpose

Vouch turns verified, resolvable attestations into a viewer-relative judgment
for a stated purpose. It helps a community decide whether to admit a steward,
rely on a claim, recognize a succession, or weight an objection. It never
changes whether an input is structurally valid or cryptographically verified.

## Required properties

1. **Viewer-relative.** Evaluation consumes one explicit store view and one
   explicit policy. There is no global reputation value.
2. **Purpose-specific.** Trust in one capacity does not transfer to another
   unless a policy says so.
3. **Partial-safe.** Missing or unresolved evidence is reported as context
   debt. Absence is never negative evidence.
4. **Explainable.** A result identifies every counted, ignored, challenged,
   withdrawn, and unresolved input and the policy clause applied to it.
5. **Non-prescriptive.** Communities sign policies; evaluators implement the
   language. Conformance never requires adopting the reference policy.
6. **Issuer-bounded.** Repetition by one issuer cannot simulate independent
   testimony. Policies cap contribution per issuer and evidence class.
7. **Withdrawal-aware.** A later signed disposition may stop an earlier input
   contributing without pretending to erase or revoke its signature.
8. **Propagation-bounded.** Trust paths are optional, anchor-rooted,
   cycle-free, depth-limited, and establish issuer eligibility only. They do
   not multiply evidence.
9. **Reproducible.** Evaluation fixes `as_of`, policy ID, engine version, and
   store-view digest. An optional signed receipt can preserve the result.
10. **Deterministic.** Equal query, policy, store bytes, and engine version
    produce the same outcome and trace.

## Outcome model

- `trusted`: the policy's positive predicate passes and its negative predicate
  does not.
- `rejected`: the negative predicate passes and the positive predicate does
  not.
- `contested`: both predicates pass, decisive evidence is challenged, or
  competing disposition heads prevent a unique active state.
- `awaiting-context`: neither predicate passes, or unresolved context could
  change the result.

Insufficient positive evidence is `awaiting-context`, never `rejected`.

## Empirical basis

The Continuity Trial demonstrates a useful trust practice: name the
irreducible trust point, verify everything around it, and preserve a trace of
who judged what. The original community simulator demonstrates endorsement
farming, delayed activation, selective harm, cover, recovery, and faction
subversion. The spatial simulator demonstrates that two honest viewers can
reach different judgments because they hold different stores.

The reference profile must outperform flat positive/negative tallying against
the simulator's adversaries without materially excluding honest newcomers.
