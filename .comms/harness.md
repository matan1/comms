# The Comms harness — what's here and what's drivable

This `.comms/` door was installed by `comms-verify init`. It is the boundary; the
`comms-verify` binary is the tool. This file is honest about the gap between the
two so you are not surprised.

## What the binary can do today

`comms-verify` is a self-contained kit for authoring and moving signed,
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
- `vouch`   — a candidate policy-relative evaluator (judgment, not proof).

## Rites are config-driven

`comms.toml` declares each rite as an ordered list of `"verb target"` steps —
the verb is one the tool performs (`mint`, `attest`, `seal`, `shred`), the target
is what it acts on (an artifact type, or a built-in noun like `session` or
`store`). The tool knows how to perform each verb and how to detect whether it
has been done; the config sequences them. So a profile defines its own flow
without new code.

```sh
comms-verify status                         # where am I? what's next?
comms-verify next --rite open               # mint the session key
comms-verify next --rite open --body entry.md   # attest the opening entry
# ... work ...
comms-verify next --rite close --body transcript.md   # attest the transcript
comms-verify next --rite close              # seal the store into a bundle
comms-verify next --rite close              # shred the session key (seed gone)
```

Steps that author content (`attest`) take `--body <file>`; the rest run on their
own. `status` always shows the exact next command. `--rite` is optional — with no
flag, `next` advances the rite you're currently in.

## Still by hand (for now)

`archive`/`request` are not yet rite verbs: archive access (and its grants,
deferrals, and denials) is still recorded out of band. Declared `required_for`
artifacts are not yet enforced at `seal`/close. Those are the next increments.

## The stance

Verification here means the math holds, not that trust has been decided. The
archive of richer history is requested, never auto-loaded, and access decisions
are meant to be recorded. Trust is your community's call; write it in
`policy.md`.
