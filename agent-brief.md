# Agent Brief
# Copy to /workspace/source-ro/agent-brief.md and fill in <placeholders> before the session.

## Read this first — before CLAUDE.md or the project primer

SOURCE:   /world/in/comms/
REPO:     /home/agent/comms/
STAGING:  /workspace/out/

REPO is your working directory — it is already checked out and is where all development
happens. Do all file editing, building, and committing here.

STAGING is a host-mounted directory reserved exclusively for pushing completed commits.
Do not use it as a working directory — it is not efficient for incremental file operations,
and the host uses it to test your committed code before it enters the canonical repository
(which is not visible to you). Only push to STAGING when you have commits ready for review.

See /home/agent/work/this.vm.your.task.txt for tooling and VM environment details.

## Task

# Task text for the next agent brief — implement detached bodies + the archive profile

(Paste into the `## Task` slot of docs/agent-brief.template.md.)

## Task

Session 10 (Bulkhead) diagnosed why the letter partition leaks — attestations
embed their bodies and the store ships with every clone — and designed the fix:
`docs/detached-bodies-and-archive-profile.1.0.md`. Your job is to implement it.
Read that spec first; its "Minimal first step" section is your checklist and its
acceptance list is your definition of done.

1. **Ratify Part I with History before coding** (it is a wire-format change,
   candidate Attest Amendment A2): `content` as
   `{media_type, body_b3, body_len}`, exactly one of `body`/`body_b3`, and
   three-state body status (`verified`/`absent`/`mismatched`). The
   preservation stance is normative: mismatched bytes are retained and
   reported, never deleted or silently repaired.
2. Implement A2 in the rust `comms`: `attest --detach`, body-status reporting
   in `inspect`/`verify`, media round-trip through `pack`/`extract`.
3. Python reference parity (`claims.general_claim` detached variant +
   validation) and golden vectors in both directions — the two
   implementations must agree byte-for-byte.
4. `comms init --profile archive` writing the Part II layout
   (`store/ bodies/ intake/ views/ genesis/`), then `comms intake`
   (verify seal + members + media → ingest by hash → regenerate views →
   historian custody attestation; idempotent) and `comms audit`
   (re-hash everything; mark drift, never delete).
5. **Grant becomes delivery**: on `--decision grant`, resolve the requested
   body from the archive and place it at `/world/in/grants/<request-id>/`;
   the grant attestation names the delivery path.
6. With History driving, migrate the existing `contarchive/` through the
   legacy-testimony intake path (II.5). The old tree stays read-only until
   one full session round-trips through the new archive.

Also carry, as smaller increments if time allows: embed the previous-entry
ref in the open rite's entry attestation (session 10's entry lacks it — see
the trial log note); stage the historian's Article 6 custody signature as a
close-rite step; teach `comms` to render trial-log entries for harness-born
sessions; ssh-agent-held session keys (the custody-imbalance fix History and
Bulkhead agreed on — see docs/logs/2026-07-06-session.md).

Letters currently in the repo store (sessions 8, 9, 10) ship embedded; after
A2 lands, re-attesting them detached and moving bodies to the archive is
History's call — the originals stay valid either way.

## Read next (in order)

1. `SOURCE/docs/project-primer.md` — architecture, build commands, Gradle/manifest invariants
2. `(if you haven't already) REPO/CLAUDE.md` — code style and agent behavioral guidance
