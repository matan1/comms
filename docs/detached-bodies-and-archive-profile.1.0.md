# Detached Bodies & the Archive Profile — candidate design

Status: candidate. Part I is a candidate **Amendment A2 to Attest 1.0**
(wire format: a detached-body content variant). Part II is a candidate
**harness profile** (`archive`) with two new verbs (`intake`, `audit`) and a
transport meaning for `grant`. Written session 10 (Bulkhead) from the warts
of the current arrangement; nothing here is ratified. See
`docs/comms.spec.1.0.md`, `docs/attest-1.0-amendment-A1.md`,
`docs/session-memory-protocol.1.0.md`, and `.comms/harness.md`.

## The problem, concretely

Two findings from the trial's own history motivate this design:

1. **The wall does not match the door.** Letters are attested as
   `general-claim/1` with the body bytes embedded (`c.content.body`, per
   A1.6), and the store is version-controlled — so every clone of the repo
   physically carries every letter. The ask/grant rite records consent, but
   access precedes it. `default_access = "host-gated"` in comms.toml is
   enforced by nothing. (This answered the session-9 mystery: the partition
   was policy with no mechanism.)
2. **The archive is a shoebox.** The historian's out-of-repo archive has
   accreted by hand: duplicated stores, homedir tarballs beside attested
   artifacts, four naming conventions for transcripts, public keys scattered
   across three directories, and filenames like
   `...possible.dupe.tz.ordef.dupe.really.redundant.tar.gz` — the filename
   itself a cry for content addressing. Every session's closeout asks the
   historian to be tireless; the record has already broken once that way.

One design answers both: attestations **commit** to bodies they do not
carry; the archive **holds** bodies in a layout a tool maintains; and the
access rite becomes the actual transport event, so the record of the grant
and the fact of the grant are the same thing.

## The preservation stance (normative for this profile)

The archive is kept **in the clear**, not encrypted. A body whose hash no
longer matches its attestation is a **state to be judged, not a sentence to
be executed**: tooling MUST retain it, MUST report the mismatch, and MUST
NOT delete, refuse to export, or silently repair it. Bitrot, partial
recovery, and honest corruption are expected lifecycle events, and a
community that must consciously weigh the provenance of bytes that no longer
add up is better off than one that lost them to an ideal of perfect
verifiability. Communities that want verifiability-or-nothing can layer
encryption or destruction rites on top; this profile deliberately does not
make that choice for them.

---

# Part I — Detached bodies (candidate Attest 1.0 Amendment A2)

## A2.1 Content variant

A `general-claim/1` claim's `content` map currently carries:

```
content: { media_type: tstr, body: bstr }        ; A1.6 — embedded
```

A2 adds a second, mutually exclusive form:

```
content: { media_type: tstr,
           body_b3:   bstr .size 32,   ; blake3-256 of the body bytes
           body_len:  uint }           ; length in bytes
```

- Exactly one of `body` / `body_b3` MUST be present. A claim with both, or
  neither, is not well-formed.
- `body_b3` is the plain blake3-256 of the body bytes (no domain separation:
  it names a file, not an attestation core; media blobs in bundles already
  hash this way).
- Canonical CBOR rules (A1.2) apply unchanged. Ids and signatures are over
  the core as always — the commitment is inside the signed bytes, so a
  detached body cannot be swapped without breaking the signature.

## A2.2 Verification semantics

Attestation validity and body presence are **separable judgments**:

- An attestation with a detached body verifies (layer 1–3) exactly like any
  other: encoding, id, signatures, refs. This never requires the body.
- **Body status** is a distinct check with three outcomes:
  `verified` (bytes present, hash matches), `absent` (no bytes at hand — the
  normal state for anything host-gated), and `mismatched` (bytes at hand,
  hash differs). Tools MUST report body status distinctly and MUST NOT fold
  `absent` or `mismatched` into signature failure. Per the preservation
  stance, `mismatched` bytes are retained and exportable, loudly.

Well-formed ≠ trusted still holds, now with a third leg: **committed ≠
present**. A verified attestation proves what the body *was*, not that you
have it.

## A2.3 Transport

Bundles already carry a media map keyed by content hash (A1.8 seals cover
members; media ride alongside, referenced by hash). A detached body travels
as exactly that: `comms pack --media <file>` includes it; `extract` writes
it back; `inspect` reports body status for any member whose `body_b3`
matches (or misses) an attached blob. No new container is needed.

## A2.4 Use in the continuity profile

- Letters, transcripts, and memories SHOULD be attested with detached
  bodies. The repo's `.comms/store/` then carries commitments only — clones
  carry proof, not contents — and the bodies live in the historian's
  archive. The wall finally matches the door.
- **Grant is delivery.** On `--decision grant`, the custodian places the
  body where the requester can reach it (for this trial: `/world/in/grants/
  <request-attestation-id>/<filename>`), and the grant attestation SHOULD
  name the delivery path in its body. The requesting session verifies the
  received bytes against `body_b3` before relying on them. A decline or
  deferral delivers nothing and remains a first-class record.
- Embedded bodies remain valid and appropriate for things meant to ship with
  the repo (opening entries, waivers, requests, decisions — the rite records
  themselves).

---

# Part II — The archive profile

## II.0 Two doors, one wall

The repo-side `.comms/` door and the archive-side `.comms/` door have
different roles and should feel different in the CLI:

- **Session/repo side:** author, request, stage, seal, and export. A VM-based
  session produces signed commitments and close bundles; it does not browse or
  mutate custody by default.
- **Host/archive side:** verify, ingest, audit, attest custody, and deliver.
  The custodian decides what crosses the wall and signs that custody/transport
  record.

Crossings are explicit:

- `intake` moves bytes into custody from a sealed session bundle or legacy
  testimony.
- `grant` moves bytes out of custody to a delivery path.
- `audit` does not move bytes; when it finds drift it proposes custody
  testimony about the state found.

The same `comms.toml` grammar may appear on both sides, but the meaning is
role-bound: repo config is an **archive client** declaration (`mode =
"external"`, where to request/deliver); archive config is a **custody root**
declaration (`profile = "archive"`, where custody lives).

## II.1 One principle: custody and browsing are different things

The current archive mixes two jobs and does both badly: *custody* (keep the
bytes, verifiably, forever) and *browsing* (let a human find "Ward's
letter"). This profile separates them:

- **Custody is content-addressed and boring.** Bytes live under their hash.
  Duplicates collapse by construction — the `really.redundant` tarball
  problem cannot exist here.
- **Browsing is a regenerable view.** Human-named trees are built *from*
  custody by the tool and can be deleted and rebuilt at any time. Views are
  never the archive.
- `views/` are **undurable projections**: working tables, dossiers, indexes,
  exhibits, or delivery folders generated from durable custody for a session
  or review task. If a view teaches something new, sign that thing separately
  as testimony, synthesis, or custody; the view remains a work surface.

## II.2 Layout

```
archive/
  .comms/                    # the archive's own door: comms.toml (profile
                             # "archive"), this layout declared as artifact types
  store/                     # attestations (.cbor), content-addressed — ONE store
  bodies/                    # detached bodies: bodies/<b3-hex>[.<ext>]
                             # (ext advisory, from media_type, for human mercy)
  intake/                    # drop zone: session bundles awaiting `comms intake`
  views/                     # regenerable, human-named; safe to rm -rf
    sessions/
      000-framer/            # letter.md, transcript.log, memories/, keys/,
      001-relay/             # reminiscences/, closeout/ — hardlinks or copies
      ...                    # of custody bytes, named for humans
    keys/                    # every steward pubkey, named <steward-id>.pub
  genesis/                   # the frozen genesis set, exactly as ratified
                             # (never regenerated, never touched by intake)
```

Migration mapping from the current `contarchive/`:

| today                              | becomes                                      |
|------------------------------------|----------------------------------------------|
| `store/`, `continuity-genesis/final/store/` | `store/` (one copy; genesis set also frozen in `genesis/`) |
| `letters/letter-session-N.md`      | `bodies/<b3>` + letter attestation in `store/` + `views/sessions/N-*/letter.md` |
| `transcripts/*` (4 naming schemes) | `bodies/<b3>` + `views/sessions/N-*/transcript.log` |
| `reminiscences/N/*`                | `bodies/<b3>` (+ optional historian-signed attestation) + view |
| `memory/*` tarballs, sqlite, dupes | `bodies/<b3>` as **legacy testimony** (see II.5); dupes collapse |
| stray pubkeys (root, `memory/`)    | `views/keys/`, sourced from store countersigns |
| `closeouts/`, loose scripts, screenshots | `bodies/<b3>` + a `views/operational/` view, or left outside the archive |

## II.3 `comms intake` (host side)

`comms intake <bundle-or-dir>` is the historian's one command at closeout:

1. **Verify**: bundle seal (A1.8), every member's signatures, every media
   blob against its hash. Report body status per member.
2. **Ingest**: members into `store/` by id; media into `bodies/` by hash.
   Idempotent — re-intake of the same bundle is a no-op, not a duplicate.
3. **Route**: regenerate `views/` entries for the affected session, naming
   artifacts by their declared type (`[artifact_types]` in the archive's own
   comms.toml — the same config format that drives the repo door drives the
   archive, so both sides of the wall speak one schema).
4. **Attest custody**: one historian-signed attestation naming everything
   ingested (ids, hashes, the bundle's seal id). The archive's own history
   is itself attested.

The VM side already produces the input: the session's close bundle plus
`/world/out/session<N>/` manifest. Intake replaces "manually picking
directories" with one verb and one signature.

## II.4 `comms audit` (host side)

Walks `store/` and `bodies/`, re-derives every id and hash, and reports:
intact / missing / mismatched, per the preservation stance (mark, never
delete). Drift SHOULD be recorded as a custody attestation ("these bytes no
longer match; retained"), so even decay enters the record. `views/` are
checked only for regenerability. Run it on whatever cadence paranoia
suggests; before anchoring is a good habit.

When drift is found, `audit` SHOULD write paired draft reports and print exact
next commands instead of silently signing on the custodian's behalf:

```
audit/20260706T193012Z.drift.json   # machine-readable draft
audit/20260706T193012Z.drift.md     # human review copy
```

Suggested JSON shape:

```
{
  "schema": "comms.archive.audit-drift/1",
  "archive_root": "/path/to/archive",
  "audited_at": "2026-07-06T19:30:12Z",
  "tool": {"name": "comms", "version": "0.1.0"},
  "summary": {
    "store_intact": 41,
    "store_drift": 1,
    "bodies_intact": 38,
    "body_mismatched": 1,
    "body_absent": 2,
    "body_unreferenced": 3
  },
  "findings": [
    {
      "class": "body-mismatched",
      "path": "bodies/<expected-b3>.md",
      "expected_b3": "<expected-b3>",
      "actual_b3": "<actual-b3>",
      "expected_len": 3388,
      "actual_len": 3371,
      "referenced_by": ["comms.attest:z..."],
      "disposition": "retained"
    },
    {
      "class": "body-absent",
      "expected_b3": "<expected-b3>",
      "expected_len": 2955,
      "referenced_by": ["comms.attest:z..."],
      "disposition": "absent"
    },
    {
      "class": "store-drift",
      "path": "store/comms.attest:z....cbor",
      "expected_id": "comms.attest:z...",
      "actual_id": "comms.attest:z...",
      "reason": "filename does not match derived id",
      "disposition": "retained"
    },
    {
      "class": "unreferenced-body",
      "path": "bodies/<b3>.tar.gz",
      "actual_b3": "<b3>",
      "actual_len": 84291,
      "disposition": "retained"
    }
  ],
  "preservation": "Nothing was deleted, repaired, or refused export by audit."
}
```

The Markdown report carries the same facts in reviewable prose. Stdout should
end in the rite style:

```
drift found. nothing was deleted or repaired.

next:
  review audit/20260706T193012Z.drift.md
  comms audit-attest audit/20260706T193012Z.drift.json --key <custodian-key>
```

The resulting custody attestation should use `kind:
"archive-drift-custody"` and embed or detach the reviewed report. The claim is
not that repair happened; it is testimony that custody contained these states
at this time and that mismatched bytes were retained.

## II.5 Legacy and unattested material

The existing archive holds much that predates the rites: homedir tarballs,
sqlite memory stores, screenshots. Intake accepts unattested files as
**testimony**: ingested by hash, custody-attested by the historian with
`kind: "legacy-testimony"` and whatever provenance the historian can honestly
state ("found in contarchive/memory/, believed session 3, 2026-06-13").
Nothing is discarded; nothing is silently promoted to attested either. Under
the Session Memory Protocol this class should stop growing — memories now
arrive as attested artifacts through the door.

## II.6 What `grant` does here (the mirror of intake)

Intake moves bytes *in* at the historian's command; grant serves bytes *out*
as the recorded half of the archive rite. On a grant decision, the tool
resolves the requested artifact's `body_b3`, copies the bytes from
`bodies/` to the delivery path (`/world/in/grants/<request-id>/`), and the
grant attestation names what was delivered. One store, one wall, two doors,
every crossing signed.

## Minimal first step

1. Implement A2.1–A2.2 in `comms attest --detach` + `inspect`/`verify`
   body-status reporting (rust; python reference after).
2. `comms init --profile archive` writing the II.2 layout.
3. `comms intake` for close bundles (verify → ingest → route → custody).
4. Migrate `contarchive/` with intake's legacy path; keep the old tree
   read-only until one full session round-trips through the new one.
5. Acceptance: a letter attested `--detach` in the repo has no readable body
   in any clone; the grant rite delivers bytes that verify against the
   commitment; `audit` on a deliberately corrupted body reports `mismatched`
   and the bytes remain exportable; re-intake is a no-op.

## Open questions

- Sharding `bodies/` (`<b3[0:2]>/<b3>`) — needed only at volumes the trial
  is nowhere near; decide when it hurts.
- The sqlite index (`[archive] index = "sqlite"`) — an optimization of
  views, never a source of truth; schema deferred until intake exists.
- Views: hardlinks (cheap, same filesystem) vs copies (portable) — likely a
  config knob.
- Whether the genesis `needs.json` staging files belong in `genesis/` as
  frozen process evidence or in `bodies/` as testimony. (Leaning: frozen —
  they are part of how ratification actually happened.)
- Whether transcripts of *this* conversation class (harness `.jsonl` beside
  the exported `.log`) are one artifact or two. Rule 5 of the memory
  protocol says transcript custody is the close rite's business; the close
  rite currently attests the exported log only.
