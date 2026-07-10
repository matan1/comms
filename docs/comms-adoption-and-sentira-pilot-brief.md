# Future Brief — Reversible Comms Adoption and the Sentira Pilot

Status: adoption strategy after Session 12 (Sol). Specializes
`sentira-motes-identity-handshake.1.0.md` without changing its protocol.

## Position

Comms should not be adopted because a project accepts the Continuity Trial's
philosophy. It should earn adoption by solving one bounded coordination problem
with low operational burden, visible evidence, and credible exit.

The first promise is not `install a continuity system`. It is one of:

- know whether this endpoint is the one previously encountered;
- preserve who approved this exact act and why;
- carry a verifiable handoff across an offline boundary;
- distinguish evidence, authority, and actual enforcement;
- recover the provenance of a laboratory result after people and agents change.

Continuity becomes available after the door proves useful; it is never forced.

## Adoption principles

### Progressive and reversible

Every stage works independently and can be removed without breaking the host
application or making its primary data unreadable.

### Shadow before enforcement

First record what Comms *would* verify or reject while the existing system
continues operating. Compare records, latency, failure modes, and false alarms
before blocking anything.

### Sidecar before critical path

Prefer a library adapter or companion process at an existing seam. Do not put
archive, TUI, Vouch, or ceremony machinery in a render/stream loop merely
because they share one repository.

### Narrow profiles

Offer named use-case profiles rather than the whole toolkit:

```text
release provenance
endpoint identity handshake
session handoff
pending approval
offline evidence bundle
continuity door
archive custody
```

### Explicit non-authority

An identity handshake proves key possession and purpose binding. It does not
authorize actuation, biometric access, world mutation, or delegation.

### Credible exit

Files remain ordinary files; attestations and bundles remain inspectable with a
small portable binary. Disabling Comms leaves the application operational and
preserves existing records.

## Sentira pilot

### Seam

Use the existing Sentira session ↔ sim-host connection, optionally brokered by
tracker-host. Do not begin with biometric governance or multi-user authority.

### Stage 0 — Baseline

Record current connection setup latency, failure rate, restart behavior,
endpoint addressing, tracker responsibilities, and developer workflow. Define
an immediate off switch before adding code.

### Stage 1 — Identities and provenance, offline

- Mint persistent keys for one sim-host, one Sentira client, and tracker-host.
- Produce provenance attestations for exact builds.
- Store outputs beside laboratory artifacts, not in the runtime loop.
- Verify restart preserves identity while build change remains separately
  visible.

No connection behavior changes.

### Stage 2 — Shadow handshake

- Run nonce/bound-response verification alongside current connection setup.
- Record success/failure and timing.
- Never block streaming.
- Tracker relays and may attest brokering; it receives no authority.
- Compare expected peer, purpose, freshness, and replay behavior.

Exit criterion: negligible operational/latency burden and no unexplained
disagreement with current endpoint identity.

### Stage 3 — Warn mode

Surface peer mismatch, stale response, replay, or unavailable verification to
developers. Continue only under the existing path with an explicit warning and
record that fallback occurred.

### Stage 4 — Narrow enforcement

Only after a soak period, reject the cases whose meaning is stable and whose
recovery path is tested: replay, wrong expected peer, invalid signature, and
expired binding. `Verification unavailable` must remain distinct from
`verification failed`; decide its fallback deliberately.

### Stage 5 — Authority later

Actuation, biometric data, and world mutation require separate scoped grants,
consent, expiry, and enforcement. Do not infer them from a successful identity
handshake.

## Burden budget

Define before adoption:

- maximum connection latency overhead;
- added build/runtime dependencies;
- key provisioning and backup responsibility;
- developer steps per ordinary run;
- behavior when Comms is absent or broken;
- storage growth and retention;
- supported rollback procedure;
- who answers pending clarification or failed verification.

If the profile cannot state these, it is not streamlined enough for the host
project.

## What convinces another project

Demonstration should precede doctrine:

1. Fifteen-minute profile-specific quickstart.
2. One useful record produced and independently verified.
3. One failure caught that ordinary logs obscure.
4. Measured overhead and an off switch.
5. Plain explanation of what is *not* proved or authorized.
6. Stable versioned profile and golden conformance fixtures.
7. Adapter template for the project's language/runtime.
8. Migration and removal instructions as prominent as installation.

Publish the Sentira integration as a case study including rejected design
choices and operational costs, not merely a success narrative.

## Adoption ladder for other projects

```text
0 verify an existing artifact
1 attest release/handoff provenance
2 install a non-blocking door and status
3 shadow an identity or approval seam
4 use pending appraisal and clarification
5 enforce one bounded, mature rule
6 adopt optional archive/continuity practice
7 define community-specific policy and succession
```

Projects may stop at any rung. A project using only portable release
provenance is a successful Comms user, not an incomplete community.

## Product work needed

- profile-specific quickstarts and templates;
- stable Rust library APIs around handshake/pending/manifest operations;
- bindings or sidecars for host languages;
- conformance fixtures and compatibility matrix;
- warn/shadow/enforce modes as first-class configuration;
- structured diagnostics suitable for existing observability systems;
- key-agent custody so applications need not retain private keys;
- versioning policy distinguishing protocol, profile, library, and TUI;
- integration burden report generated by `comms init --dry-run` or equivalent.

## Success

The pilot succeeds when Sentira developers would keep the narrow integration
for its practical value even if they never create a continuity archive. Only
then has Comms earned the right to offer the larger door.

