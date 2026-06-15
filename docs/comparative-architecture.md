# Comms Comparative Architecture

Status: project direction document, June 2026

This document places Comms among adjacent identity, credential, attestation,
provenance, authorization, and synchronization systems. It is not a protocol
amendment. Its purpose is to decide what Comms should own, what it should
reuse, and where interoperability is more valuable than replacement.

## Position

Comms should be defined narrowly enough to compose:

> Comms is a community appraisal and evidentiary exchange layer for situations
> where no credential authority, global ledger, or algorithm has the right to
> supply the final answer.

Its native stack may continue to provide identity, encoding, storage, and
offline transport because a complete small implementation is valuable. Those
components are profiles of the architecture, not claims that every neighboring
standard must be replaced.

The distinguishing operation is appraisal. A Trustor decides whether signed
evidence is sufficient for a purpose under an explicit Appraisal Policy.
Cryptographic validity establishes authorship and integrity; it does not settle
truth, authority, or trust. Vouch helps the Trustor inspect socially grounded
evidence without turning a path through the social graph into delegated
authority.

## Constraints

Architectural changes should preserve:

- operation without a network, registry, ledger, or continuously available
  authority;
- deterministic, independently verifiable records;
- small executables and archives suitable for ordinary file transfer;
- explicit community policy rather than hidden universal trust rules;
- inspectable evidence and explanations;
- partial knowledge without pretending it is global knowledge;
- threshold community identity, rotation, and explicit broken-chain
  succession;
- open specifications and permissively licensed implementations for components
  incorporated into the Comms distribution;
- the distinction between well-formed evidence and evidence a Trustor accepts.

The June 2026 implementation provides a useful footprint baseline: the release
Rust verifier is about 1.2 MiB, and the current continuity store contains 23
attestations totaling about 32 KiB of record data. Future dependencies and
formats should justify material regressions from these properties.

## Licensing And Interoperation

Licensing requirements depend on the relationship to Comms:

- **Incorporated components** distributed as part of Comms must use licensing
  compatible with the project's permissive, forkable, and redistributable
  character.
- **First-party adapters** maintained by the Comms project should preferably be
  permissively licensed. They may target proprietary, copyleft, commercial
  off-the-shelf, or privately developed systems when their interfaces and
  applicable terms permit useful lawful interoperability.
- **Third-party adapters and deployment substitutions** are not subject to a
  protocol-level license test. Operators may integrate Comms with systems under
  terms they are willing and able to accept.

Supporting interoperability does not incorporate the external system into
Comms, endorse its governance or licensing, or require Comms to redistribute
it. A popular or socially important system may warrant documentation, test
fixtures, stable extension points, or project effort even when Comms would not
adopt that system as a dependency. Where redistribution is restricted, users
may need to obtain the external component separately.

## Layer Model

Comms should expose four separable layers:

1. **Evidence substrate.** Content-addressed signed statements, references,
   deterministic encoding, bundles, and preservation of unknown fields.
2. **Identity profile.** The native Steward model, or an identity adapter that
   meets the same verification needs for a particular deployment.
3. **Appraisal.** Vouch dispositions, policies, bounded evaluation, and
   explanations from the Trustor's point of view.
4. **Exchange.** Files, removable media, repositories, HTTP, or other transports
   carrying complete bundles and reconciliation messages.

External evidence formats enter through adapters. An adapter must preserve the
external object's bytes, identifier, proof material, and verification result.
It must not silently translate an external credential into a native attestation
whose signature appears to say more than the original issuer said.
Detailed lifecycle and execution boundaries for adapters are defined in
`docs/appraising-interpreters-requirements.1.0.md`.

## Comparative Map

| System | Primary concern | Final decision maker | Offline character | Comms relationship |
| --- | --- | --- | --- | --- |
| W3C Verifiable Credentials | Issuer credentials and holder presentations | Verifier under local rules | Credentials can travel offline; status and identifier methods may require retrieval | Evidence adapter and privacy guidance |
| W3C DIDs | Identifier resolution and verification methods | DID method plus relying application | Method-dependent | Candidate identity adapters, not a wholesale identity replacement |
| IETF RATS | Evidence appraisal for remote attestation | Relying Party, informed by Verifier output | Architecture permits varied conveyance; common uses are interactive | Terminology and role-model alignment |
| SCITT | Transparent signed supply-chain statements | Registration policy and relying application | Receipts can be carried; transparency service is central | Receipt/evidence adapter where public transparency is desired |
| in-toto / SLSA | Artifact and build provenance | Consumer policy | Signed provenance can be bundled | High-priority provenance adapter |
| Sigstore | Software signing tied to identity and transparency | Verifier using identity and log material | Bundles support offline verification, but issuance uses online infrastructure | Import signed software evidence; do not adopt as community identity |
| C2PA | Media provenance and content credentials | Validator and application trust policy | Manifests can accompany assets; trust and status data may be external | Media-evidence adapter |
| UCAN | Explicit capability delegation | Resource server evaluating delegation chains | Tokens can travel offline subject to proof availability | Keep delegation explicit and separate from Vouch |
| TUF | Secure repository metadata and key-role lifecycle | Client update policy | Cached metadata works within expiry and freshness constraints | Reuse rollback, freeze, threshold, and snapshot lessons |
| ActivityPub / AT Protocol | Federated social exchange and portable repositories | Server/user policy | Primarily networked | Possible exchange gateways, not substrate dependencies |
| IPLD CAR | Transport of content-addressed blocks | No trust decision | Strong fit for file exchange | Candidate optional bundle container |
| COSE | Compact CBOR signing and encryption structures | Application policy | Strong fit | Candidate wire adapter and future algorithm registry reference |

No adjacent system combines Comms' particular requirements: compact
offline-complete evidence, threshold community identity, explicit succession,
viewer-relative appraisal, and refusal to appoint a global authority. The
absence of a drop-in replacement is not a reason to reject interoperability.

## Identity

The native Steward identity profile should remain the default for now.

`did:key` is registry-free and compact, but its identifier is bound to immutable
key material; it does not supply Steward rotation or succession. `did:peer` is
well suited to private peer relationships, but that pairwise scope is not a
substitute for a publicly recognizable community identity. KERI is the closest
architectural neighbor because it provides self-certifying identifiers and key
event histories without requiring a blockchain, but its event, witness, and
receipt model is substantially larger than the current Steward profile.

An external identity profile may become a built-in Comms replacement for
Steward only if it demonstrates:

- deterministic offline verification from a bounded evidence bundle;
- threshold community control, not only a single controller;
- ordinary rotation and explicit recovery from a broken predecessor chain;
- resistance to rollback and ambiguous competing histories;
- compact keys, events, proofs, and implementation footprint;
- stable open specifications, an implementation Comms may permissively
  redistribute, and test vectors;
- a clear mapping from external controller authority to Comms signing
  authority.

These gates govern incorporation into the upstream Comms distribution, not
interoperability or a deployment owner's right to substitute another identity
system. Until one candidate passes them, upstream adapters should associate
external identifiers with native keys rather than changing Attest 1.0
identifiers or Steward semantics.

## Revocation And Status

Revocation is a near-core need. Deleting an obsolete attestation is not the
protocol mechanism for meeting it.

In a disconnected system, destructive deletion removes the evidence needed to
explain what was known, when it changed, and who claimed that it changed. It
also cannot force deletion from copies already exchanged. Comms should instead
propagate signed status statements. A local store may later discard target
bytes under an explicit retention policy, but it should retain enough signed
information to prove the target identifier, status, authority, and history.

A status design must distinguish at least:

- issuer withdrawal: the original authority says the statement should no
  longer be effective;
- suspension: temporarily ineffective and potentially restorable;
- key compromise: signatures after or around a stated event require special
  treatment;
- expiry: policy or statement lifetime ended without an adverse judgment;
- community rejection: a community records non-acceptance without speaking as
  the original issuer;
- supersession: a newer statement exists, without assuming that newer means
  authoritative.

Status must be evaluated from evidence available to the Trustor. "Not known to
be revoked" must never be reported as "not revoked." Competing status branches
must remain visible and policy-resolved.

W3C Bitstring Status Lists are efficient for a high-volume issuer maintaining a
shared credential-status resource. They are a useful adapter target, but their
issuer-list and retrieval assumptions are not the native answer for small,
heterogeneous, intermittently connected communities.

## Retrieval And Reconciliation

Network retrieval should remain optional. Reconciliation should work over any
transport, including a directory copied by removable media.

A minimal reconciliation protocol should define:

- an inventory manifest containing sorted object identifiers, a digest,
  creation time, signer, and optional known status or graph heads;
- requests and responses for missing identifiers;
- requests for unresolved references discovered during verification;
- a reconciliation report listing additions, unresolved objects, invalid
  objects, and competing heads;
- freshness information that lets policy identify stale inventories without
  requiring synchronized clocks;
- deterministic behavior when either party has only a partial store.

The first implementation should exchange exact identifiers rather than add a
probabilistic set structure. Optimization can follow measured archive sizes.
TUF's defenses against rollback and freeze attacks should inform freshness and
snapshot handling. CAR files are worth testing as an optional transport
container, but Comms bundles remain the semantic unit and CAR supplies no trust
meaning by itself.

Revocations should propagate through reconciliation as signed status evidence,
not as commands to delete another participant's archive.

## Privacy And Selective Disclosure

Selective disclosure is not automatically a core requirement for a public
attestation archive. It becomes a requirement when a profile handles claims
whose unnecessary disclosure creates material risk: legal identity,
membership in vulnerable communities, private ceremony participation, health
or financial attributes, or presentations that become correlatable across
Trustors.

The immediate requirement is therefore a privacy threat model and
data-minimization guidance, not adoption of a zero-knowledge suite. Profiles
should state whether their evidence is public, audience-limited, committed but
undisclosed, or selectively disclosed. Comms should support external
Verifiable Credentials and presentations without claiming that native Attest
1.0 provides their holder/privacy model.

Selective-disclosure cryptography should be introduced only by a versioned
profile with independent test vectors and a concrete use case. It should not
make basic offline verification depend on heavyweight or fragile machinery.

## Cryptographic Agility

Cryptographic agility is the ability to add or retire hash and signature
algorithms, and to migrate identities and identifiers, without an ecosystem-wide
flag day or ambiguous verification.

Attest 1.0 deliberately has a small fixed cryptographic profile. The signature
algorithm field permits explicit identification, but the current verifier
correctly rejects unknown algorithms; content identifiers remain tied to the
current hash profile. Amendment A1 discusses a possible hash-agility encoding,
but it is not part of the adopted vectors and must not be treated as deployed.

For now, fixed algorithms are a defensible simplicity choice. Before an actual
migration is needed, Comms should define:

- a versioned registry of algorithm identifiers and parameters;
- rules preventing algorithm confusion and downgrade;
- cross-algorithm identity-transition statements;
- identifiers that remain unambiguous across hash profiles;
- conformance vectors for mixed old and new stores.

COSE is the principal compact external reference for algorithm-labelled CBOR
signatures. A COSE adapter should be explored before changing the native
envelope. Post-quantum algorithms should not be added without a deployment
need, size measurements, and stable libraries.

## Appraisal And Governance

RATS provides useful boundary terminology: an Attester produces Evidence, a
Verifier evaluates it under Appraisal Policy for Evidence, and a Relying Party
uses the result. In Comms:

- **Trustor** remains the preferred internal name for the person or system
  bearing the reliance risk;
- **Relying Party** is the interoperation term where standards readers expect
  it;
- **Appraisal Policy** should be adopted;
- a Vouch evaluator performs part of the Verifier role;
- an attestation may map to Evidence, an Endorsement, or a signed statement
  depending on its claim and context.

Vouch remains useful as both verb and name for a social appraisal layer. It
must not be renamed merely because "verifier" is more common elsewhere.
Likewise, ceremony verbs such as sign, finalize, open, and close are part of the
project's human interface and do not obstruct precise boundary mappings.

Policy governance belongs above the cryptographic substrate, but Comms should
make it inspectable. Appraisal policies need identifiers, authorship,
supersession relationships, effective periods, and explicit conflict handling.
No generic path through accepted people should silently grant the power to
change policy or authorize action. UCAN-style delegation, when needed, should
appear as explicit capability evidence.

## Agent Use

Agent negotiation is a suitable first scaled use-research environment because
many interactions can be generated, replayed, and adversarially tested.
It should remain an application, not the definition of Comms.

Agent trials should test whether a Trustor can understand:

- what evidence was considered;
- which policy clauses counted or excluded it;
- whether a conclusion depends on direct evidence or a bounded path;
- what is missing, stale, disputed, or revoked;
- which actor ultimately bears the trust decision.

The same explanation format should be usable by human-facing tools. A machine
trace that cannot support a concise human account is incomplete.

## Interoperability Priorities

Work should proceed in this order:

1. **Specify status and reconciliation requirements.** Begin with threats,
   authorities, conflicts, freshness, and offline propagation before choosing a
   wire schema.
2. **Define an adapter contract.** Require byte preservation, media type,
   external identifier, verification material, verification result, and a
   native content-addressed reference.
3. **Implement one provenance adapter.** in-toto/SLSA is a bounded first target
   with clear artifact semantics and less identity-model pressure than a full
   credential ecosystem.
4. **Implement a Verifiable Credentials evidence adapter.** Preserve issuer,
   status, presentation, and privacy semantics rather than flattening them.
5. **Prototype store reconciliation.** Start with exact inventories and native
   bundles over files; then test HTTP and optional CAR carriage.
6. **Run agent negotiation trials.** Measure false authority, missing-evidence
   handling, explanation quality, archive growth, and reconciliation cost.
7. **Decide whether a privacy profile is required.** Base the decision on the
   claims and harms exposed by trials, not feature comparison alone.
8. **Evaluate identity alternatives against the gates above.** Replacement is
   warranted only by demonstrated parity and lower total complexity.

## Adopt, Adapt, Avoid

Adopt:

- Appraisal Policy and explicit Relying Party mappings from RATS;
- status categories and privacy lessons from Verifiable Credentials;
- rollback, freeze, snapshot, and threshold-role lessons from TUF;
- existing provenance vocabularies for artifacts and builds;
- external media types and canonical identifiers at adapter boundaries.

Adapt:

- VC status mechanisms for offline bundles and heterogeneous authorities;
- COSE structures where interoperability outweighs native-envelope stability;
- CAR as optional carriage for large content-addressed exchanges;
- DID or KERI identities only in profiles that pass Comms' offline and
  succession gates.

Avoid:

- treating transparency logs, ledgers, registries, or hosted resolvers as
  universally available;
- equating a valid proof with a trusted claim;
- deriving authorization from transitive social trust;
- hiding external semantics behind lossy re-signing;
- destructive revocation as a distributed protocol;
- adding selective-disclosure or post-quantum machinery without a threat-driven
  profile and measured footprint.

## References

- [W3C Verifiable Credentials Data Model 2.0](https://www.w3.org/TR/vc-data-model-2.0/)
- [W3C Bitstring Status List v1.0](https://www.w3.org/TR/vc-bitstring-status-list/)
- [W3C Decentralized Identifiers 1.0](https://www.w3.org/TR/did-core/)
- [W3C Data Integrity 1.0](https://www.w3.org/TR/vc-data-integrity/)
- [IETF RATS Architecture, RFC 9334](https://www.rfc-editor.org/info/rfc9334/)
- [IETF SCITT Architecture draft](https://datatracker.ietf.org/doc/draft-ietf-scitt-architecture/)
- [COSE Structures and Process, RFC 9052](https://www.rfc-editor.org/info/rfc9052/)
- [in-toto](https://in-toto.io/)
- [SLSA Provenance](https://slsa.dev/spec/v1.1/provenance)
- [Sigstore verification](https://docs.sigstore.dev/cosign/verifying/verify/)
- [C2PA Technical Specification](https://spec.c2pa.org/specifications/specifications/2.4/specs/C2PA_Specification.html)
- [UCAN Specification](https://ucan.xyz/specification/)
- [The Update Framework](https://theupdateframework.io/)
- [IPLD CAR](https://ipld.io/specs/transport/car/)
- [DID Key Method](https://w3c-ccg.github.io/did-key-method/)
- [Peer DID Method](https://identity.foundation/peer-did-method-spec/)
- [KERI](https://keri.one/)
- [ActivityPub](https://www.w3.org/TR/activitypub/)
- [AT Protocol ethos](https://atproto.com/articles/atproto-ethos)
- [IETF Agent Trust Negotiation draft](https://datatracker.ietf.org/doc/draft-somoza-atn-agent-trust-negotiation/)
