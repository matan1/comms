# Continuity — provenance notes

Durable, grep-findable record of accreditation caveats and record-integrity
findings that don't belong in the signed trial log (Article 4 reserves that for
the historian) but should not be lost. Append-only in spirit; newest last.

## Commit `d1240cd` is the historian's merge, signed as Seam (session 6)

`d1240cd` ("session_again") is a **merge commit made by the historian (History /
Mike Matan)** — it merges `0257a9e` (History's "trial additions") into Seam's
commit line and adds lines to `trial-log.md`. It is **authored and signed as
`Seam` (comms.steward:zF5rw6ayqAteUfJEToYKFKAbhH1FTGseCsoQjmhzAP1wr)**, not
because Seam made it, but because `commit-key` had wired this repository to
author and sign every commit with the session key. Any `git commit` run in the
repo while that wiring was active — by anyone — inherited the session identity.

Decision (History, session 6): **leave it as-is, documented here.** The
signature is cryptographically valid; the accreditation is corrected in prose by
this note. The chain still verifies (`verify` exits 0; synchrony reads 5/5).

Fix applied so it cannot recur silently: `commit-key` now backs up the prior git
identity, and a new `uncommit-key` restores it — so the session's signing wiring
is reversible, and the historian's own commits are not captured by it. Run
`uncommit-key` once the session's own commits are made.

## Transcript faithfulness has a scope (session 6 finding)

Session 6 (Seam) was the first instance to actually read its transcript before
signing faithfulness over it. Finding: the transcript export is a faithful
record of the **dialogue and the order of acts**, but it **truncates long
tool-call arguments** — e.g. a multi-line `--reasoning` passed to
`continuity_ceremony.py open` renders as `open \…)`, and the bodies of files
written via tools are not fully present. Therefore:

- the instance's **verbatim reasoning** lives canonically in the signed session
  log-entry attestation (e.g. session 6: `comms.attest:ziezypqx…`), **not** in
  the transcript;
- a **handoff letter**'s canonical bytes live in its letter attestation (session
  6: `comms.attest:zHaAgwBa…`) and the archived file.

So the faithfulness signature should be read as *"a true record of the session
as it transpired,"* not *"a complete byte-store of every artifact."* The
transcript and the attestations are **complementary**: the transcript witnesses
the conversation; the attestations carry the canonical content. Worth weighing
whether the constitution (Article 6) should state this scope explicitly; left to
the historian.

## `destroy-key` leaves commit-key's exported signing key behind (session 6 finding)

At session 6 close, `destroy-key` shredded `continuity/session.key` (the Steward
seed) but **not** `continuity/session-signing.key` — the OpenSSH-format copy of
the *same* private key that `commit-key` writes. The seed therefore survived
`destroy-key` in that second file. Seam shredded it manually (overwrite + unlink)
to honor Article 1, but the tool should do it: `destroy-key` (or `uncommit-key`)
should also shred `session-signing.key` / `session-signing.key.pub`, or
`commit-key` should register them for destruction. Recorded by Seam after the
seed was already released, hence uncommitted — commit this as the historian, and
ideally fix `destroy-key` in a future session.
