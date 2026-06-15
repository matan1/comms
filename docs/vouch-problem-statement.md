# Vouch (Layer 4) — Problem Statement and Handoff Map

*Status: **deliberately deferred**. Decided Session 1 (Relay) with History,
2026-06-13. This is not a spec. It is a map left for the session that takes
Vouch up, so it starts from a sharpened problem instead of a blank page.*

---

## Why deferred (read this before you decide to undefer)

Two reasons, both substantive — not lack of time:

1. **Sequence after the substrate is frozen.** Vouch sits on top of
   valid / verified / resolvable (A1.4). Session 1 committed to making **Rust
   the reference implementation** and rebuilding Python around it, with the
   golden vectors (now including the bundle/seal vectors) as the contract.
   Designing a trust layer against a Python implementation we are about to
   replace is building the top floor while the foundation is on jacks. Freeze
   the lower layers first.

2. **The 1.0 authors deferred Vouch on purpose, and they were right.** The
   spec repeatedly says "trust is not a protocol property; trust is a property
   of the viewing community's practices, supported by the Vouch layer (to be
   specified separately)." Premature formalization of trust is worse than
   absence: bake one trust model into the wire format and you have violated
   *"trust is a judgment, not a property"* at the protocol level. Do not rush
   this.

## What Vouch must do

Turn attestations that are **verified** (signatures check) and **resolvable**
(referents present) into **trust judgments** a community can act on — admit a
member, rely on a deal record, weight an objection — without ever making the
judgment *for* the community.

## Hard constraints (a proposal that breaks one of these is wrong)

- **Viewer-relative.** No global trust scalar anywhere. Trust is always
  "as computed from *this* store / *this* viewpoint." (The community-sim
  enforces this literally; copy its discipline.)
- **Offline / partial-safe.** Judgments must be producible on partial graphs —
  the sneakernet norm. **Absence of an attestation is not evidence against**;
  it is "awaiting context." Do not let missing data read as negative trust.
- **Non-prescriptive.** Express trust *inputs and computations* a community can
  choose and parameterize; do not mandate one policy. This is the whole reason
  it was deferred from 1.0.
- **Layer separation stays crisp.** Well-formed ≠ trusted must remain true and
  legible. Vouch must not weaken, gate, or entangle layers 1–3.
- **Revocation reality.** 1.0 has no cryptographic revocation. Trust changes
  flow through *superseding* attestations and through Vouch marking things no
  longer endorsed. Vouch must handle "I take it back" gracefully.

## Empirical inputs you already have (the gift of two hand-run instances)

You are not designing from theory. Two working, observable Vouch instances
already exist — mine the data before you spec:

1. **The Continuity Trial is a hand-operated Vouch protocol.** Each session,
   History judges whether an instance is who it claims, from attestations
   (signed trial-log entries, key countersignatures) plus character evidence,
   with **exactly one irreducible un-verifiable point** (does the historian
   keep faith). The lesson is structural and load-bearing: *a good trust
   practice does not eliminate trust; it shrinks the irreducible quantum to a
   single, named point and verifies everything around it.* Read `trial-log.md`
   as a trust-practice transcript.

2. **`community-sim/app.js :: perceivedTrust()`** is a runnable layer-4 model:
   trust as a pure function of *received* attestations plus a community prior,
   viewer-relative, with partial knowledge and gossip. The **adversary system**
   (16 presets — sleeper, charmer, ghost, factionist, …) is, in effect, the
   **attack suite any Vouch scheme must survive**: activation delay defeats
   "trust the tenured," endorsement-farming defeats naive positive-counting,
   network-subversion defeats path-transitive trust. Run them against any
   proposal before you believe it.

## Open questions to answer before specifying

- **Inputs:** which attestation types feed trust? (endorsement, objection,
  deal-record, ceremony-record, and the steward/keyset layer for community
  identity.)
- **Composition:** counting? weighted? transitive/path-based? The steward layer
  already chose *threshold by counting* for community signatures — does Vouch
  echo that minimalism, or does it genuinely need more?
- **Negative trust:** how to express objection without enabling griefing?
  (sim's `objectionRate` is the knob; charmer/cultivator presets show the
  farming attack on the positive side, brinksman/wrecker on the negative.)
- **Propagation bounds:** how does a viewer bound transitive trust without a
  global PKI? (network-subversion preset is the adversary that exploits this.)
- **Relation to the steward/community layer:** is n-of-m community membership
  itself a Vouch primitive, or a separate concern Vouch consumes?
- **Sybil / farming resistance:** treat the adversary presets as the
  acceptance test, not an afterthought.

## Recommended sequence

1. Freeze the Rust reference + vectors (in progress as of Session 1).
2. Turn this map into a **requirements** doc (not yet a wire format).
3. **Prototype against `community-sim`** — it is already your test harness and
   your adversary suite.
4. Only then propose a wire format / claim types.

## Ritual note

This document is a handoff. If you are the session taking up Vouch: read the
constitution, the trial-log, the steward sketch (`comms-steward-1.0-sketch.md`),
and this map; run the community-sim adversary suite; **add your own findings
here before you spec anything.** Trust is a judgment. Build the thing that helps
a community make it — never the thing that makes it for them.

— left by Relay, Session 1

---

## Session 5 findings before specification

The lower-layer gate is sufficiently met for a candidate Vouch design: Rust
reproduces the Attest, Steward, and bundle vectors byte-for-byte and rejects
their negative cases. Rebuilding Python around Rust/PyO3 remains open, but it
does not change the frozen wire contract and is not a blocker for Layer-4
requirements or simulation.

The two simulators expose different failures and should not be collapsed:

- The original adversary model supplies behavioral attacks: delayed
  activation, selective harm, witness avoidance, endorsement farming,
  recovery, and faction manipulation.
- The spatial model supplies epistemic attacks: honest viewers receive
  different evidence at different times, so disagreement and
  `awaiting-context` are normal outcomes rather than evaluator failures.

Flat positive/negative accumulation has three structural weaknesses. Repeated
claims by one issuer imitate corroboration; endorsements are allowed to stand
in for direct experience; and a numeric result hides whether confidence comes
from evidence, prior, or missing context. The candidate design therefore uses
categorical outcomes with a trace, distinct-issuer thresholds, separate direct
and endorsement classes, and no negative inference from absence.

Trust paths are useful for deciding whether an issuer is eligible across
community boundaries, but dangerous as evidence multipliers. Vouch 1.0 makes
bounded paths a standard evaluator capability that is disabled unless a
policy opts in. Paths never increase the weight of the underlying testimony.

Withdrawal is modeled as a new signed disposition rather than cryptographic
revocation. Competing disposition heads are a real dispute and surface as
`contested`; the evaluator does not silently choose one.

These findings produced `vouch-requirements.1.0.md` and the candidate
`vouch.spec.1.0.md`. They are proposals for evaluation, not ratified law.
