# Comms Source Assessment

Date: 2026-05-28

## Project Summary

Comms is a small Python toolkit for community-grounded attestations in
experimental agent networks. It models stewards as Ed25519 identities, wraps
claims in deterministic CBOR attestation envelopes, signs and verifies those
envelopes, stores them by content ID, and provides higher-level ceremonies for
agent provenance, capability proof, admission, guardianship, and recognition.

The project is useful as a prototype substrate for auditable coordination among
AI agents, services, or mixed human/agent communities. A community can record
who an agent is, where it came from, what capabilities it demonstrated, who
vouched for it, how it was admitted, and how shared resources were allocated.

## Implemented Surface

- `identity.py`: single-key steward identities using Ed25519.
- `canonical.py`: deterministic CBOR, BLAKE3 hashing, and base58btc multibase
  identifiers.
- `attest.py`: attestation envelope, content-addressed IDs, signing,
  serialization, and well-formedness checks.
- `store.py`: in-memory and directory-backed content-addressed attestation
  store with simple graph helpers.
- `claims.py`: claim constructors for agent provenance, capability,
  membership, resources, endorsements, recognition, and action records.
- `ceremony.py`: runnable rites for capability challenge/proof, provenance,
  admission, guardianship, and recognition.
- `allocate.py`: a convivial resource allocator using seed floor, peer vouching,
  capability scores, audit inputs, and per-agent caps.

## Dependencies

`requirements.txt` now records the current Python runtime dependencies:

- `cbor2`
- `PyNaCl`
- `blake3`

The local VM can import `cbor2` and `PyNaCl`; `blake3` is missing. The VM also
does not currently have `pip` installed for `/usr/bin/python3`, so installing
from the requirements file was not verified here.

## Spec And Demo Context

`docs/comms.spec.1.0.md` defines the Attest 1.0 substrate:

- deterministic CBOR wire format,
- content-addressed attestation IDs,
- Ed25519 signatures over the canonical core hash,
- general claim/frame/ref/signature structure,
- preservation of unknown claim types and ref roles,
- sneakernet bundle format for offline transport,
- explicit distinction between well-formedness and trust.

The `../../coopete_demo` directory contains a simple end-to-end demo using the
same modules. It shows a community steward, an allocator, several agents,
capability rites, guardianship for a micro agent, resource requests,
endorsements, allocation, grant return, graph verification, and tamper
detection.

## Design Assessment

The core architectural idea is sound: keep the protocol substrate small and
tamper-evident, then let communities define their own trust practices. The code
already reflects this distinction. It can verify signatures, hashes, refs, and
wire structure, but it does not claim that a well-formed attestation is trusted.

That split is important for the intended use case. Comms should support both
high-tech and low-tech settings, including offline, underground, or degraded
infrastructure environments. A protocol-level object should be portable across
USB drives, local disks, message buses, QR/paper workflows for small bundles,
and future network transports. Community trust rules should remain local and
adaptable.

## Protocol-Level Guarantees

These should be treated as substrate guarantees independent of local community
custom:

- Attestations are tamper-evident.
- IDs are deterministic and content-addressed.
- Signatures bind claims, frames, and refs to steward keys.
- Unknown claim types and ref roles are preserved.
- Offline and online transports can carry the same attestations.
- Viewers can distinguish "well-formed" from "trusted".

## Community-Level Policy

These should remain configurable by each community:

- who counts as a valid sponsor,
- how many sponsors or witnesses are required,
- whether attestations expire or need scheduled renewal,
- what ceremony template is sufficient for identity binding,
- how objections and superseding attestations are handled,
- how delegated authority, guardianship, and recovery work,
- whether external timestamps, media records, or physical artifacts are needed.

## Threat Model Framing

For this project, a threat model means documenting what failures or attacks the
substrate is designed to tolerate and what is left to community policy. Useful
questions include:

- What happens if a steward key is stolen?
- How are old attestations replayed, renewed, superseded, or objected to?
- How should a viewer handle missing refs in an offline bundle?
- What happens when two communities disagree about the same steward or claim?
- How does a node handle conflicting histories received months apart?
- How does the system resist tampered bundles, partial bundles, or junk floods?
- Which timestamps are merely signed assertions, and which have external
  evidence?

The protocol does not need to answer every policy question directly. It should
provide enough structure for communities to express and audit their answers.

## Readiness Criteria To Develop

Likely next milestones:

1. Core verifier for canonical encoding, IDs, signatures, refs, and preservation
   rules.
2. Bundle read/write support matching the Attest 1.0 sneakernet format.
3. A pluggable policy layer for required signers, sponsor thresholds, renewal
   windows, objection handling, and local trust decisions.
4. Safer key stewardship guidance beyond development-grade raw seed files.
5. Demo scenarios for offline exchange, contested attestations, supersession,
   key rotation, and missing/partial bundles.
6. Tests that compare canonical hashes and signature behavior against known
   vectors, ideally including the referenced Rust implementation.

## Immediate Gaps

- No packaging metadata beyond the new `requirements.txt`.
- No automated tests.
- No bundle implementation yet.
- No CLI or user-facing verifier.
- Multi-key/community stewards are identifiable but not yet verifiable.
- Typed constructors do not cover all Attest 1.0 claim types.
- Trust policy is currently implicit in ceremony code and demo conventions.
