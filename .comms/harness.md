# The Comms harness — what's here and what's drivable

This `.comms/` door was installed by `comms init`. It is the boundary; the
`comms` binary is the tool. This file is honest about the gap between the
two so you are not surprised.

## What the binary can do today

`comms` is a self-contained kit for authoring and moving signed,
content-addressed attestations offline:

- `status`  — read this door and report **where you are in the rite and the next
              step**, in prose or `--json`. Start here.
- `next`    — perform the next pending step of the active rite (mints, attests,
              seals, or shreds as the rite declares). The config-driven workflow.
- `init`    — install or refresh this door.
- `mint`    — generate a steward key (`{seed_b58, label}` JSON, mode 0600).
- `attest`  — author **and sign** a `general-claim/1` attestation from a content
              file (a letter, a transcript, a memory, any statement). This is how
              you create a primary artifact; the result is an `<id>.cbor`.
- `pack`    — gather `.cbor` attestations (and/or `--media` blobs) into a bundle.
- `seal`    — add an A1.8 integrity seal (signs the exact member set).
- `verify`  — check a bundle's seal.
- `inspect` — verify every member on its own terms (signatures, refs, media).
- `extract` — write a bundle's members and media back out to files.
- `sign`    — countersign staged pending items with an OpenSSH ed25519 key or a
              steward key. The counterparty's half of a rite: run it wherever
              their key lives; needs naming other keys are left standing.
- `finalize`— verify fully-signed pending items and move them into the store
              under their content id. Aborts loudly on anything unsigned.
- `waive`   — record, under the session key, that a `required_for` artifact
              cannot be produced this session. The gap becomes an attestation.
- `vouch`   — a candidate policy-relative evaluator (judgment, not proof).

## Rites are config-driven

`comms.toml` declares each rite as an ordered list of `"verb target"` steps —
the verb is one the tool performs (`mint`, `attest`, `seal`, `shred`,
`countersign`, `request`, `grant`), the target is what it acts on (an artifact
type, or a built-in noun like `session` or `store`). The tool knows how to
perform each verb and how to detect whether it has been done; the config
sequences them. So a profile defines its own flow without new code.

```sh
comms status                         # where am I? what's next?
comms next --rite open               # mint the session key
comms next --rite open --body entry.md   # attest the opening entry
comms next --rite open               # stage the key countersign (with [countersign])
#   ...the counterparty: comms sign --key ~/.ssh/id_ed25519 && comms finalize
# ... work ...
comms next --rite archive --body ask.md      # record an archive request
comms next --rite archive --key <their key> [--decision grant|decline|defer]
comms next --rite close --body transcript.md   # attest the transcript
comms next --rite close              # seal the store into a bundle
comms next --rite close              # shred the session key (seed gone)
```

Steps that author content (`attest`, `request`) take `--body <file>`; `grant`
takes the counterparty's `--key` (a decline or deferral is a first-class
record, not a failure); the rest run on their own. `status` always shows the
exact next command and any staged item still awaiting a signature. `--rite` is
optional — with no flag, `next` advances the rite you're currently in.

## Requirements are enforced, gaps are recorded

Declared `required_for` artifacts are enforced at `seal`: a rite does not seal
while a required artifact is neither attested nor waived. A session that
cannot produce one records the gap instead of being blocked —
`comms waive <type> --body <reason>` — and the waiver is itself a
session-signed attestation (honored only where the rite sets
`allow_waivers = true`).

## The session key can live in memory only

By default the session seed is a file (`session.key`, destroyed at shred). A
file that outlives a crashed session is a liability: the next session's open
rite reads as done, and the key could sign as its dead owner — `status` warns
whenever a key is sitting on disk. Set `session_key = "ephemeral"` in
`comms.toml` and the seed never touches disk at all: `mint` shows it exactly
once, later steps read it from `COMMS_SESSION_SEED` in the holder's
environment (it must derive the recorded session id, so a wrong seed cannot
quietly sign), and the shred is the holder unsetting and forgetting it — the
tool reports the step done only once no environment can produce the seed. A
crashed ephemeral session's seed dies with it. The trade: the seed passes
through the holder's memory and environment, so a holder whose own context is
recorded (e.g. an agent transcript) should prefer the file mode and shred at
close.

## The stance

Verification here means the math holds, not that trust has been decided. The
archive of richer history is requested, never auto-loaded, and access decisions
are meant to be recorded. Trust is your community's call; write it in
`policy.md`.
