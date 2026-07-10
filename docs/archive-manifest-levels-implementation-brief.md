# Future Brief — Complete the Archive Manifest Ladder

Status: implementation handoff after Session 12 (Sol). Read
`archive-manifest-and-pending-workbench.1.0.md` and the manifest code in
`rust/archive.rs` first.

## Current state

Implemented:

- `comms manifest <root> --level minimal|full`;
- one deterministic inventory snapshot shared by both projections;
- minimal aggregate disclosure without artifact paths, hashes, names, titles,
  excerpts, or rankings;
- full artifact metadata, BLAKE3, exact duplicates, attestation refs/support,
  signature validity, and structured gaps;
- archive-profile snapshots restricted to durable `store/`, `bodies/`, and
  `genesis/` (never `views/` or generated output);
- legacy roots inventoried as found and marked structurally unappraised;
- an optional, labeled 256-byte custodian threshold line outside the inventory
  commitment;
- minimal/full browsing in `comms-tui`.

The remaining ladder is `door` and `interpretive`, plus deeper full-manifest
semantics.

## Objective 1 — Keep the door fixed

Do not turn `door` into a dynamic archive summary. The Session Memory Protocol
makes the stub the ceiling for automatically injected context.

If `comms manifest --level door` is added, it should render only fixed protocol
capabilities already present in `.comms/door.md`:

- an archive may exist;
- no archive body or manifest was loaded;
- minimal/full/reading-path/full-archive requests are available;
- names remain optional and chosen;
- where to inspect the door policy.

It must not scan custody, include counts, carry the custodian threshold line,
or vary when the archive changes. A byte-stable golden fixture should pin it.

## Objective 2 — Enrich the full projection without guessing

The current full projection derives what ordinary files and parseable
attestations reveal. Extend it from archive-profile custody records and intake
views rather than filename folklore.

Add, where evidence exists:

- artifact type declared by intake;
- session steward id, session number, chosen name, and substrate, each with its
  source attestation;
- embedded/detached body form and body status;
- custody intake id and originating bundle seal;
- access class and delivery/grant relationships;
- clarification, appraisal, supersession, challenge, and disposition edges;
- legacy provenance and confidence wording exactly as attested;
- duplicate-of as a byte relation, distinct from two independent custody acts;
- explicit `unknown` rather than inferred metadata.

Every derived field should carry either `source: <attestation-id>` or
`derivation: <mechanical-rule>`. Do not collapse signature validity, body
presence, custody, faithfulness, or trust.

## Objective 3 — Define interpretive overlays

Use an ordinary detached `general-claim/1` initially:

```text
kind: archive-interpretive-overlay
about: <manifest snapshot id>
role: interpreter
body media type: application/json
```

Candidate body:

```json
{
  "schema": "comms.archive-overlay/1",
  "snapshot": "comms.manifest:z...",
  "title": "Boundaries that learned to hold",
  "position": "one reader's proposed constellation",
  "nodes": [
    {
      "artifact": "comms.attest:z...",
      "body_b3": "...",
      "citation": {"byte_start": 120, "byte_end": 418},
      "excerpt_b3": "...",
      "annotation": "Bulkhead distinguishes possession from grant."
    }
  ],
  "edges": [
    {"from": 0, "to": 1, "relation": "complicates"}
  ],
  "counterpoints": [2],
  "reading_paths": [
    {"name": "short", "nodes": [0, 1, 2], "selection_rule": "..."}
  ]
}
```

Transcript citations should use source body hash plus byte offsets and excerpt
hash. Line numbers are presentation hints only: JSONL normalization, wrapping,
and transcript exports can change line boundaries.

An overlay is invalid against a snapshot when a cited artifact/body cannot be
resolved or an excerpt hash fails. It may remain valid testimony against an
older snapshot; do not silently retarget it.

## Objective 4 — Render projections, not one giant JSON page

Add full-manifest TUI projections over the same model:

- chronology;
- lineage and rites;
- work threads;
- custody and wall crossings;
- substrates and identity lifecycle;
- gaps/incidents;
- interpretive overlays.

Selection in any projection should focus the same artifact in every other
projection. Always expose source/derivation in the detail pane. Search results
are filters, not rankings unless a ranking rule is explicitly selected.

## Tests and acceptance

- Door output is byte-stable and invariant under archive changes.
- Minimal and full continue to share an inventory snapshot.
- Changing the custodian line changes presentation but not custody snapshot.
- Writing any manifest under `views/` cannot change its snapshot.
- Every enriched field names its source or mechanical derivation.
- Overlay citation/excerpt verification has positive and negative vectors.
- Missing overlay context reports `awaiting-context`, not invalid history.
- No interpretive overlay appears automatically in a cold session.

## Non-goals

- No authoritative archive summary.
- No default Virgil or popularity ranking.
- No claim that an inventory is historically complete.
- No migration of custody into a view or SQLite-only index.

