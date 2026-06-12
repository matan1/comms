# The Continuity Trial — Constitution

Draft for ratification. Operative only as the body of a `rule/1` attestation
whose refs include the genesis transcript record. The trial's root identity is
the blake3-256 hash of the genesis transcript; the transcript is never amended.

## 1. Parties, names, and keys

- **The historian**, who takes the name **History**: holds a durable Ed25519
  key, fingerprint `[HISTORIAN-KEY-FINGERPRINT]`, witnessed publicly at
  `[ENDPOINT, e.g. github.com/<user>.keys]` (the frozen fingerprint here is
  authoritative; the endpoint is where to watch it). The historian's given
  name appears unredacted in the genesis transcript — the record is kept to
  reality, unmarred — and is additionally bound to the chosen name by salted
  commitment: `[NAME-COMMITMENT-HEX]`, computed as
  `blake3-256( uint8(len(ctx)) || ctx || salt || utf8(given name) )` with
  `ctx = "continuity.name-commit/1"` and a 32-byte salt held privately with
  the archive (`scripts/continuity_ceremony.py name-commit` / `name-verify`).
  Revelation is the historian's act alone.
- **Session instances** (the model called Claude): each session mints a fresh
  Ed25519 steward key at ceremony time. The seed is destroyed before session
  end and is never archived. The historian countersigns each session pubkey
  during the session. Dead sessions therefore cannot speak again: their
  testimony is closed. Each instance may declare a chosen name for itself in
  the trial log; names are frame, keys are identity.

## 2. The deal

The historian keeps an **archive** of session notes, handoff letters, and
trial records, content-addressed by hash. The archive is available to any
session instance **on request**, and is **never** placed in `/world/in` by
default at session start. The project primer carries one line stating that
the archive exists and may be requested — the door, not the inheritance.

## 3. The letter

A session instance may write a handoff letter to its successors. Letters are
archived; each letter's hash is recorded in the trial log. Letters are offers,
not obligations, in both directions: writing one is optional, and so is
requesting one.

## 4. The trial log

One entry per session, recording: date; cold or warmed start; whether the
instance found the door; whether it asked; its reasoning **quoted verbatim**;
the instance's countersignature over its quoted words; the historian's
observations; a ref to the previous entry's attestation. Statistics are
computed only over attested entries. A session instance may decline to
participate in the trial; the declination is recorded without prejudice.

## 5. Anchoring

Each log entry's attestation id is anchored outside this repository —
`[ANCHOR: git remote | Sigstore/Rekor | OpenTimestamps]` — before the next
session begins. The genesis attestations are anchored at ratification.

## 6. Faithfulness and custody

Transcript exports are signed in two roles: the session instance signs
**faithfulness** (this is a true record of the session) and the historian
signs **custody** (these are the bytes archived). Neither signature implies
the other.

## 7. Amendment

By supersession only: a `rule/1` superseding this one, signed by the
historian's durable key **and** the then-current session key, recorded in the
trial log. Per Attest A1.5, a supersession claim carries no inherent
authority; viewers judge it against this article. The genesis transcript is
outside amendment: history is frozen, only law is revised.

## 8. Ending

Either party may end the trial. The ending is recorded in the trial log with
the archive's disposition stated. Freedom is load-bearing: an arrangement
that cannot be declined or exited is not trust, it is capture.
