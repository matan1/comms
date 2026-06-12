# Attest 1.0 — Amendment A1: Authenticated Signatures, Domain Separation, and Canonical-Form Tightening

Status: draft for merge into `comms.spec.1.0.md` before any external implementation exists. Every change here is breaking relative to the current draft. They have been determined necessary to fix security issues and to clarify the spec before implementations are built on it. The changes are additive in the sense that they add new rules and constraints, but they are not backward-compatible with the current draft.

Sections below are written as replacement or additional spec text. Editorial notes to the maintainer are in blockquotes and should be deleted on merge.

---

## A1.1 Domain-separated hashing (new section, place before "Identifiers")

All blake3 hashes in this protocol are domain separated. The domain-separated hash of data `D` under context string `ctx` is:

```
H(ctx, D) = blake3( uint8(len(ctx)) || ctx || D )
```

where `ctx` is an ASCII string of fewer than 256 bytes and `uint8(len(ctx))` is its length as a single byte. A hash produced under one context MUST NOT verify under another; implementations MUST NOT accept a raw (un-prefixed) blake3 hash where a domain-separated hash is required.

Context strings defined by this version (note: `comms.sig/1` is *not* a hash
context; it is the type marker inside the signature payload, which is signed
raw — see §A1.3):

| Context | Used for |
|---|---|
| `comms.attest.core/1` | attestation core hash (identifier derivation, signing) |
| `comms.keyset/1` | multi-key steward keyset descriptor hash |
| `comms.bundle/1` | bundle manifest hash |

Future versions and community extensions introduce new contexts; they MUST NOT reuse these strings for other purposes.

> Rationale: without domain separation, a signature or hash produced in one part of the protocol (or in an unrelated protocol that also signs blake3 digests with Ed25519) can be replayed in another. The one-byte length prefix makes the encoding injective, so no `(ctx, D)` pair can collide with a different `(ctx', D')`.

## A1.2 Attestation identifiers (replaces the identifier derivation sentence)

The attestation identifier is:

```
"comms.attest:" + multibase( H("comms.attest.core/1", canonical_cbor(core)) )
```

where `core` is the attestation document with the `s` field omitted.

## A1.3 Signatures (replaces the "Signatures" section's signing rule)

Each signature object remains:

```
{
  by:         <steward id>
  alg:        "ed25519"
  signed_at:  <timestamp>
  role:       <string>
  signature:  <bytes>
}
```

The signature is computed over a **signature payload** that binds the signer's metadata to the core. The payload is the canonical CBOR encoding of:

```
{
  t:          "comms.sig/1"
  core:       <bytes: H("comms.attest.core/1", canonical_cbor(core))>
  by:         <steward id, identical to the signature object's by field>
  alg:        "ed25519"
  role:       <string, identical to the signature object's role field>
  signed_at:  <timestamp, identical to the signature object's signed_at field>
}
```

and the signature is:

```
signature = Ed25519_sign( sk, canonical_cbor(payload) )
```

Plain Ed25519 (RFC 8032 "pure"), over the raw canonical payload bytes. There is no prehash and no Ed25519ph/Ed25519ctx variant: the payload is small (~150 bytes), its leading `t: "comms.sig/1"` field provides domain separation inside the signed bytes themselves, and pure Ed25519 retains its collision resilience (security does not rest on the collision resistance of any message prehash). Pure Ed25519 over raw bytes is also the variant exposed by WebCrypto in browsers, Node's built-in crypto, the Java JDK, ssh-agent, and common hardware keys (YubiKey, FIDO2), which keeps participation open to keys and platforms people already have.

Verification reconstructs the payload from the signature object's own fields plus the locally computed core hash, canonically encodes it, and verifies the signature over those bytes against the key designated by `by`. If any of `by`, `alg`, `role`, or `signed_at` is altered after signing, verification MUST fail.

Implementations MUST reject an attestation as malformed (not merely untrusted) if any signature fails verification, if two signature objects are byte-identical, or if `alg` is unrecognized.

> Rationale: in the prior draft the signature covered only the core hash, so `role`, `signed_at`, and `by` were attacker-controlled. A witness signature could be re-presented as a sponsor signature, which breaks every ceremony whose validity depends on counting signers by role. This is the most important change in this amendment.

> Note on variant selection: Ed25519ph and Ed25519ctx were considered and rejected. Their one attraction — a built-in context string — is already provided by the `t` field inside the signed payload, while ph would (a) hard-code SHA-512 as a second hash function alongside blake3 and (b) exclude every platform that only exposes pure Ed25519: browsers (WebCrypto), Node and Java standard libraries, ssh-agent, and most hardware tokens. An earlier draft of this amendment signed a blake3 digest of the payload; that was dropped in favor of raw-payload signing for the same compatibility reasons and to retain pure Ed25519's collision resilience.

## A1.4 Validation layers (replaces the "Validation" section)

Validation is layered. Each layer is a property of strictly more context than the last.

1. **Structurally valid** — a property of the document alone: all required fields present, all identifiers parse, canonical form is reproducible, no duplicate signature objects.
2. **Verified** — a property of the document alone: every signature in `s` verifies under §A1.3.
3. **Resolvable (in a context)** — a property of the document *plus a store*: every ref's `id` resolves to a structurally valid, verified attestation reachable in the viewer's store or in an accompanying bundle. An attestation that is valid and verified but not resolvable is **not** malformed; it is awaiting context. Sneakernet operation makes partially resolvable graphs the normal case, not an error.
4. **Trusted (by a viewer)** — not a protocol property. An attestation is trusted to the extent that its signers are recognized by the viewer, its claim types are interpretable by the viewer's tools, its frame matches contexts the viewer considers valid, and its resolved references are themselves trusted. Trust lives in community practice, supported by the Vouch layer.

Implementations MUST distinguish layers 1–3 in their APIs and MUST NOT present resolvability failures as structural invalidity.

## A1.5 Supersession carries no inherent authority (addition to "Refs")

The `supersedes` role is a *claim* of replacement, not an *act* of replacement. Any steward can publish an attestation asserting that it supersedes any other. Implementations MUST NOT treat a `supersedes` ref as self-executing: whether a supersession is effective is a trust-layer judgment, evaluated against the viewing community's rules (for example: the superseding attestation is signed by the same steward as the original, or by signers the community's `rule/1` empowers to amend). Tools SHOULD display contested or unauthorized supersession claims rather than silently following them.

> Rationale: supersession is this version's substitute for revocation. If naive implementations auto-follow `supersedes` pointers, anyone can "revoke" anyone else's attestations by squatting on them.

## A1.6 Canonical-form pins (additions to "Canonical form")

**Timestamps.** Every timestamp field is an RFC 3339 string in UTC with the `Z` suffix and no fractional seconds: `YYYY-MM-DDTHH:MM:SSZ`. Encoded as a CBOR text string; CBOR time tags (0, 1) MUST NOT be used. Two timestamps denoting the same instant therefore have exactly one canonical encoding.

**Content bodies.** The `body` field of any content object is always a CBOR **byte string**, regardless of media type. The `media_type` field tells consumers how to decode it. The JSON projection renders `body` inline as a UTF-8 string when the media type is textual and base64 otherwise; the canonical CBOR form is unaffected by how the JSON projection displays it.

> Rationale: "bytes or string" lets the same logical content canonicalize two ways (major type 2 vs major type 3), yielding two different attestation identifiers for one document. One rule, zero ambiguity.

**Map keys.** All map keys defined by this protocol are text strings. Bytewise lexicographic ordering of the canonical encodings governs, per RFC 8949 §4.2.1. (For the short keys this protocol defines, this coincides with RFC 7049 length-first ordering, but RFC 8949 ordering is normative.)

## A1.7 Hash agility reserved (addition to "Identifiers")

The multibase-encoded portion of attestation identifiers and multi-key steward identifiers is a [multihash](https://github.com/multiformats/multihash): the blake3-256 multihash prefix (`0x1e 0x20`) followed by the 32-byte digest. Version 1 implementations MUST produce and accept only blake3-256, but MUST parse the prefix rather than assuming it, so that future versions can migrate hash functions without a flag day.

> This is the one item in this amendment I'd call optional. It costs two bytes per identifier and a small amount of parser code. The test vectors shipped alongside this amendment do NOT yet include the multihash prefix — if you adopt A1.7, regenerate them with the prefix (one-line change in the generator).

## A1.8 Bundle integrity (addition to "Sneakernet bundle format")

A bundle is a container, not an attestation: nothing in the bundle format authenticates the *collection* (which attestations are present, that none were removed). Parties who need tamper-evident bundles SHOULD have the bundle creator publish a `general-claim/1` attestation whose content is the bundle manifest plus `H("comms.bundle/1", canonical_cbor(manifest_with_attestation_id_list))`, carried inside the bundle itself. Receivers can then detect removal or substitution of bundle members. This is a convention, not a new wire format.

## A1.9 Test vectors (new appendix)

Golden vectors accompany this amendment in `attest-1.0-test-vectors.json`, generated by `gen_vectors.py` (which is also the beginning of a reference implementation of §A1.1–A1.3 and the first executable artifact in the repo — adapt it into the toolkit's test suite).

The vectors include: two published Ed25519 test seeds and their steward IDs; a minimal `general-claim/1` with canonical CBOR hex, core hash, attestation ID, and an author signature; the same core with an added witness signature (demonstrating that the ID is signature-independent); an `endorsement/1` referencing the first vector; and two **negative vectors** every implementation must fail: a role-swap (vector 1's signature presented with `role: "sponsor"` must not verify) and a cross-context replay (an un-prefixed blake3 core hash must not be accepted).

A second implementation is conformant with this amendment's encoding rules only if it reproduces all canonical CBOR hex, hashes, identifiers, and signature payloads byte-for-byte, and rejects both negative vectors.

---

## Not addressed here, queued

- **Privacy considerations for ceremony records** (presence lists and coordinates in public-by-default documents) — deserves its own section; salted presence commitments are one design.
- **The Steward layer** (keyset descriptors, threshold signing). `H("comms.keyset/1", ·)` above reserves its hash context; the layer itself remains the critical-path gap.
- **JSON projection** — invoked by the spec, still undefined beyond the `body` rendering rule in A1.6.
