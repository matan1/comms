# Attest 1.0 — Amendment A2: Detached Bodies

Status: **ratified** 2026-07-06 (History, with session 11/Manifest), from Part I
of `docs/detached-bodies-and-archive-profile.1.0.md` (drafted session 10,
Bulkhead). Unlike A1 this amendment is backward-compatible: every attestation
well-formed before A2 remains well-formed and keeps its identifier. A2 adds a
second, mutually exclusive content form and a verification vocabulary for it.

Sections are written as additions to `comms.spec.1.0.md`.

---

## A2.1 Detached content variant (addition to `general-claim/1`)

A `general-claim/1` claim's `content` map carries one of two forms:

```
content: { media_type: tstr, body: bstr }        ; A1.6 — embedded

content: { media_type: tstr,
           body_b3:   bstr .size 32,   ; blake3-256 of the body bytes
           body_len:  uint }           ; length in bytes
```

- Exactly one of `body` / `body_b3` MUST be present. A content map with both,
  or neither, is not well-formed (a layer-1 structural failure, per A1.4).
- A content map with `body_b3` MUST also carry `body_len`; a content map with
  `body` MUST NOT carry `body_len` or `body_b3`.
- `body_b3` is the **plain** blake3-256 of the body bytes — no domain
  separation. It names a file, not an attestation core; media blobs in
  bundles already hash this way, so one body has one name everywhere.
- Canonical CBOR rules (A1.2, A1.6) apply unchanged. Identifiers and
  signatures are over the core as always — the commitment is inside the
  signed bytes, so a detached body cannot be swapped without breaking the
  signature.

## A2.2 Body status (addition to A1.4's validation layers)

Attestation validity and body presence are **separable judgments**. An
attestation with a detached body passes layers 1–3 exactly like any other;
none of those layers ever requires the body bytes.

**Body status** is a distinct check with three outcomes:

- `verified` — bytes at hand; blake3-256 matches `body_b3` and length matches
  `body_len` (for an embedded body: trivially verified, the bytes are the
  commitment).
- `absent` — no bytes at hand. The normal state for anything host-gated; not
  an error, not a signature failure.
- `mismatched` — bytes at hand under the body's name, but the hash or length
  differs.

Implementations MUST report body status distinctly and MUST NOT fold `absent`
or `mismatched` into structural invalidity or signature failure.

**Preservation stance (normative).** Mismatched bytes are a state to be
judged, not a sentence to be executed: tooling MUST retain them, MUST report
the mismatch, and MUST NOT delete, refuse to export, or silently repair them.
Bitrot, partial recovery, and honest corruption are expected lifecycle
events; a community that must consciously weigh the provenance of bytes that
no longer add up is better off than one that lost them to an ideal of perfect
verifiability.

Well-formed ≠ trusted still holds, with a third leg: **committed ≠ present**.
A verified attestation proves what the body *was*, not that you have it.

## A2.3 Transport (no new wire format)

Bundles already carry a media map keyed by content hash (media blobs ride
alongside members, referenced by hash; A1.8 seals cover members). A detached
body travels as exactly that: `pack --media <file>` includes it, `extract`
writes it back, and `inspect` reports body status for any member whose
`body_b3` matches (or misses) an attached blob. No new container exists.

## A2.4 Test vectors

Golden vectors for the detached variant accompany this amendment (a detached
`general-claim/1` with canonical CBOR hex, core hash, id, and signature; a
both-forms negative; a neither-form negative; and a mismatched-body case that
must report `mismatched` while the signature still verifies). A second
implementation is conformant only if it reproduces the positive vector
byte-for-byte and rejects both negatives at layer 1.
