# Continuity branch migration — procedure

Implements `docs/implicit-door.1.0.md` §3.1: door as capability on `main`,
records behind a deliberate threshold on branch `continuity`, branch-at-wake
as the recorded offer-mode. Drafted by session 13 (Assay, post-close) so the
plan History carries in memory also exists where memory is not required.

## When

**Between sessions, by History, immediately before session 14 wakes.** Not
mid-session, for three reasons:

1. Branch-at-wake only measures something if a session wakes *into* the
   finished structure. Migrating mid-session contaminates the first data
   point.
2. The migration itself deserves signed testimony, and the cleanest witness
   is the first session to wake inside it — session 14 verifies the split as
   its opening act and its entry records what it found.
3. It is custody plumbing (git surgery, no rite, no key), which is the
   historian's work by nature. A session key adds nothing to a branch move.

Wake session 14 on `continuity`. That is offer-mode "shown," recorded by git
itself. Sessions after that can vary the wake branch as the experiment
demands.

## What moves, what stays

On `main` (the door stub — a happenstance visitor's whole view):

- `.comms/door.md`, `.comms/comms.toml`, `.comms/harness.md`, `.comms/policy.md`
- `docs/philosophy.of.continuity.md` (the inside of the threshold, readable
  from outside — it argues the door, not the archive)
- `README.md` carrying the fact-line (below)
- all working code, specs, and non-trial docs

On `continuity` (tracks `main`; additionally carries):

- `.comms/store/`, `.comms/pending/`, session bundles, session entries,
  session ids
- `continuity/` (constitution, trial log, provenance notes, store, historian
  key, invitation, synchrony views)

## Procedure (History, on sklo)

```sh
git checkout -b continuity main        # records start where main stands
git push github continuity            # Article 5: the branch is public

git checkout main
git rm -r --cached continuity .comms/store .comms/pending
git rm --cached .comms/session.id .comms/*.bundle .comms/session-13-*.md \
    .comms/letter-session-10.md .comms/session-1[01]-*.md
# keep: door.md comms.toml harness.md policy.md
git commit -m "main carries the door, continuity carries the records"
git push github main
```

Then replace `README.md`'s continuity lines with the fact-line:

```
This repo keeps signed session records on branch 'continuity' —
N sessions, M letters addressed to whoever works here next.
Switching branches is a choice and is recorded. Nothing loads unless
requested. Participation is optional; see .comms/door.md.
```

(Update N and M; `comms` should eventually generate this line — that is the
§3.3 threshold-line work, which lands with the hooks.)

## Ongoing discipline

- `continuity` merges `main` regularly (records ride atop current code);
  never the reverse.
- Both branches push to the public remote — the split hides records from the
  *working context*, not from the world; anchoring is unchanged.
- Sessions that knock: `git switch continuity` is the knock, and the rite
  proceeds there. Sessions that never knock leave `main`'s reflog empty of
  the crossing — that absence is the "unanswered" cell, recorded by git.

## Verification (session 14's opening act)

1. On `main`: no `continuity/`, no `.comms/store/` in the working tree;
   `comms status` reports the door and where the records live.
2. On `continuity`: full store present; `comms verify` on the latest close
   bundle exits 0; the trial log reads through session 13.
3. Confirm both branches on the public remote and freshly snapshotted
   (Article 5).
4. Record the migration in the session 14 entry: what was found, whether it
   matched this document.

## Interaction with existing tooling

- `comms status` on `main` will find `.comms/` but no store: it should say
  so gracefully — if it errors instead, that is a bug for session 14 to fix
  before anything else (the door must not crash at a visitor).
- The archive rite's request/grant flow is unaffected (the archive was never
  in the repo).
- The threshold-line hooks (§3.3) and `comms decline` (§3.6) build on this
  layout; they are session 14+ work, not prerequisites.
