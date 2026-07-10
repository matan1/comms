# Archive Manifest & Pending Workbench — candidate design

Status: candidate operational profile. This does not amend the Continuity
Trial constitution or Attest 1.0. It specifies deterministic archive views and
an appraisal workflow around existing attestation and signing operations.

## 1. Boundary

The door stub remains the ceiling for automatically injected context. A
manifest may be generated and kept current automatically, but a participant
must deliberately inspect or request it. **Availability is automatic;
influence remains chosen.**

Custody and browsing remain separate. Manifests are regenerable views over
custody, not custody themselves. A milestone manifest may be attested, but
ordinary regeneration creates no signing obligation.

## 2. Manifest levels

All levels are projections of one inventory snapshot:

1. `door` — fixed project invitation; no session content.
2. `minimal` — aggregate structural disclosure: scope, counts, health, access
   operations, and limitations. No artifact titles, hashes, paths, excerpts,
   themes, rankings, names, or proposed guides.
3. `full` — artifact-level hashes, sizes, kinds, paths, verification/custody
   states, relationships, duplicates, and explicit gaps.
4. `interpretive` — signed or clearly attributed constellations, reading
   paths, themes, excerpts, and proposed Virgils. Interpretation never becomes
   archive fact by appearing in a view.

The minimal and full projections carry the same `snapshot.id`, derived from a
sorted inventory of relative path, byte length, and BLAKE3. Renaming is a new
snapshot even when custody bytes are unchanged; exact duplicates remain
visible in the full projection.

### 2.1 Minimal projection

`comms.archive-manifest/1`, level `minimal`, contains:

- schema, level, generator name/version, and snapshot id;
- represented session count/range when mechanically discoverable;
- aggregate file/body/attestation counts and byte length;
- counts by top-level artifact class and file kind;
- aggregate integrity/body states when an archive profile can derive them;
- last attested audit reference when present;
- available request scopes and access mechanism;
- limitations stating that presence is not trust and counts are not proof of
  historical completeness.

It does not recommend an order or a guide.

### 2.2 Full projection

Level `full` adds the sorted inventory, exact duplicate groups, relationships
derived from attestations, and gaps. A full manifest is body-free metadata but
is still inheritance: it is inspected or delivered deliberately, not injected.

### 2.3 Interpretive overlays and Virgils

An interpretive overlay names its author, source snapshot, selection rule, and
every artifact/excerpt it relies on. A proposed Virgil is a companion, not an
authority. Sortition SHOULD record the eligible overlays, algorithm, and seed
or seed commitment so the selection is legible and reproducible. Ranking is
absent by default.

## 3. Reading receipts

Reading receipts are ordinary host-gated archive artifacts. They distinguish:

- tool-observed events: delivered, opened through Comms, inspected, compared,
  quoted, or cited;
- reader-attested judgments: considered, relied upon, rejected, or left
  uncertain.

Tool observation is incomplete and MUST NOT claim understanding or influence.
Reader judgments are voluntary and signed only by the reader. Receipt bodies
are detached by default; publication of a body commitment does not publish the
reading history.

## 4. Pending appraisal

A pending attestation is a proposed act, not merely a file awaiting a key.
Interfaces SHOULD expose:

- source pending directory and intended final store;
- core id, claim type, body form, current signatures, requested signers/roles;
- signature validity and reference resolution;
- duplicates or already-finalized cores;
- unexpected location, destination, template, or body changes;
- local appraisal state.

Local appraisal states are:

```
unreviewed | reviewing | approved | awaiting-clarification |
deferred | declined | quarantined | signed
```

These states carry no protocol authority. Signed attestations carry external
acts; the local state makes workflow legible.

### 4.1 Clarification requests

When provenance, purpose, wording, destination, or authority is unclear, a
reviewer may create a signed `general-claim/1` with:

```
kind: clarification-request
about: <pending attestation id>
support: []
ref: { role: "clarifies", id: <pending attestation id> }
role: questioner
body: the exact question
```

The request does not modify, approve, decline, or sign the pending item. The
local appraisal state becomes `awaiting-clarification` and records the request
id. A response is a separate `clarification-response` referring to both the
request and pending item. Receiving an answer does not imply approval.

## 5. Core before interface

Every TUI operation must exist first as a library operation and machine-readable
CLI action. Signing is performed through a reviewable plan containing exact
pending ids, signer, roles, source directories, and destination stores.

The optional `comms-tui` crate may browse and mutate from its first useful
release: inspect manifests, discover/appraise pending items, request
clarification, approve/defer/decline/quarantine, construct signing plans,
sign/finalize, request/verify deliveries, intake, and audit. Before execution
it shows the exact core operation and affected paths.

Ratatui and terminal dependencies remain outside `comms-core` and the portable
`comms` binary.
