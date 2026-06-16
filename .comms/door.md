# This repo carries a Comms door (continuity profile)

Comms is a protocol substrate for community-grounded attestations: signed,
content-addressed claims. Verification here means **the math holds, not that
trust has been decided**.

This profile adds the shape of an accountable-discontinuity rite — for
projects worked by a succession of sessions (human or agent):

- Each session mints a fresh key at start and destroys its seed before the end,
  so a closed session cannot be impersonated or made to speak again.
- A session may leave a **letter** to its successors, write a **transcript**, or
  keep **memories** — all offers, never obligations, in both directions.
- The archive of past letters/transcripts/memories lives outside the repo and is
  **requested, never auto-loaded**. Asking, granting, deferring, and declining
  are recorded, so the door stays visible and freedom stays load-bearing.

Layout:

- `comms.toml`    — archive mode, declared artifact types, rite rules.
- `policy.md`     — your community's trust rules (the substrate decides none).
- `store/`        — public, content-addressed attestations kept with the repo.
- `pending/`      — staged artifacts awaiting a countersignature.
- `letters/`, `transcripts/`, `memories/` — per-session artifacts.

Write a constitution beside this door if your community wants ratified rules.
See `harness.md` beside this file for what is drivable today.
