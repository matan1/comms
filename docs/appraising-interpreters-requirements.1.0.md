# Appraising Interpreters: Governing What Comms Understands

Status: candidate requirements for an interpreter and execution layer. They do
not amend Attest 1.0, define a wire schema, or require Comms implementations to
execute carried software.

## Purpose

Comms must be able to retain and exchange evidence it does not yet understand.
It may later acquire an adapter, decoder, verifier, or cryptographic
implementation capable of interpreting that evidence. Acquiring those bytes
must not silently authorize either their meaning or their execution.

This layer governs how a Trustor appraises new means of interpretation while
preserving an intentionally small trusted base.

> Comms may retain and transport what it cannot understand. Interpretation
> requires appraisal; execution requires a separate grant.

## Boundary Terms

- **Format:** rules by which bytes represent a structure.
- **Transfer protocol:** rules by which bytes move between parties or stores.
- **Artifact:** content-addressed bytes retained by Comms, whether understood or
  opaque.
- **Interpreter profile:** a content-addressed specification of how a class of
  input is decoded, validated, or assigned meaning.
- **Implementation:** source, bytecode, or native executable that realizes an
  interpreter profile.
- **Appraisal Policy:** the Trustor's rules for relying on a profile,
  implementation, interpretation, or execution.
- **Confinement:** limits placed on execution, including available inputs,
  capabilities, resources, and side effects.
- **Receipt:** a signed account of a particular interpretation or execution.

A container format is not a transfer protocol. A transfer protocol is not an
interpreter. An interpreter is not necessarily executable software, and
possession of an implementation is not permission to run it.

For example, CAR defines a format for content-addressed blocks. HTTP, email, and
removable media can transfer a CAR file. A CAR reader interprets its structure.
None of these operations establishes whether the contained records are
trustworthy.

## Required Separation

Implementations MUST represent four independent operations:

1. **Retain.** Store exact bytes as an opaque, content-addressed artifact.
2. **Transport.** Send or receive exact bytes through a named transfer
   mechanism.
3. **Interpret.** Rely on a named interpreter profile to derive structure,
   verification results, or meaning from exact input bytes.
4. **Execute.** Run a particular implementation under an explicit execution
   grant and confinement policy.

The following implications are forbidden:

- Retain does not imply Interpret or Execute.
- Transport does not imply acceptance, endorsement, or freshness.
- Interpret does not imply Execute; an interpretation may come from an
  external service, prior receipt, formal derivation, or already trusted
  implementation.
- Execute does not imply reliance on the output.
- Inclusion in a valid bundle does not imply appraisal of any member.
- Popularity, number of attestations, or a Vouch path does not grant ambient
  execution authority.

These distinctions are security boundaries, not merely user-interface states.

## Retention Requirements

1. **Byte fidelity.** Retention MUST preserve exact bytes and a stable content
   identifier. Unknown artifacts MUST NOT be normalized, transcoded, or
   rewritten.
2. **Opaque carriage.** A node MAY retain and transport an artifact whose
   format, signature algorithm, or semantics it does not support.
3. **State distinction.** Tools MUST distinguish at least `unsupported`,
   `unappraised`, `accepted`, `rejected`, and `invalid` where those judgments
   can be made. Unsupported is not invalid.
4. **Metadata separation.** Locally observed media types, filenames, retrieval
   locations, and labels MUST NOT alter the artifact identifier and MUST NOT be
   confused with claims made by the artifact's author.
5. **No self-authentication.** A bundle or artifact cannot authenticate itself.
   Integrity derives from content addressing; authority derives from separately
   signed and appraised evidence.

## Transport Requirements

1. **Mechanism neutrality.** Interpreter semantics MUST NOT depend on whether
   bytes arrived by HTTP, repository synchronization, email, removable media,
   or another transfer mechanism unless the Appraisal Policy explicitly treats
   transport observations as evidence.
2. **No transport trust.** Successful transfer proves neither origin nor
   acceptance.
3. **Bound observation.** A transport receipt MUST identify its signer, the
   exact artifact identifiers observed, whether the signer sent or received
   them, the outcome, and an observation time or other ordering evidence. It
   SHOULD identify the transfer mechanism, counterparty, expected collection,
   and any omissions or failures when those facts are known.
4. **Partial-safe exchange.** A receiver MUST be able to retain a partial set
   without treating missing profiles, implementations, references, or
   attestations as negative evidence.
5. **Selective relay.** Custodians MAY specialize in retaining or transporting
   particular evidence classes. Tools SHOULD reveal the source and known
   incompleteness of a received collection so selective distribution cannot
   masquerade as a complete view.

## Interpreter Kinds

`Interpreter` is an architectural umbrella, not the preferred name for every
user-facing component. Profiles MUST declare one or more concrete kinds:

- **decoder:** derives structure from a format;
- **validator:** checks structural or semantic constraints;
- **adapter:** exposes the semantics and guarantees of an external system at a
  Comms boundary;
- **proof verifier:** verifies a proof under a named cryptographic profile;
- **status interpreter:** derives status observations from systems such as
  Bitstring Status List or Status List 2021;
- **policy evaluator:** applies a policy language to evidence;
- **transformer:** derives a new representation or artifact;
- **renderer:** produces a human-facing representation without becoming
  authoritative for the underlying meaning.

A profile may compose several kinds, but its results MUST identify which
operations occurred. A parser succeeding MUST NOT be reported as semantic
validation; semantic validation MUST NOT be reported as proof verification.

## Adapter Requirements

An adapter exists to expose the strengths and limitations of the system being
adapted, not to mimic that system with superficially similar Comms claims.

An adapter profile MUST declare:

- accepted input formats and versions;
- exact outputs and their semantics;
- which external guarantees are preserved, checked, unavailable, or lost;
- proof, identity, status, privacy, and freshness dependencies;
- whether external retrieval or a global service is normally expected;
- canonicalization and identifier rules;
- failure and uncertainty outcomes;
- conformance vectors or fixture suites;
- permissions required by implementations.

Adapters MUST preserve the original artifact or an exact content-addressed
reference to it. They MUST NOT silently re-sign transformed data in a way that
appears to transfer the original issuer's authority.

Lossy adaptation MAY be permitted by policy, but the loss MUST be explicit in
the interpretation result. Where the external system distinguishes issuer,
holder, verifier, status authority, or relying party, the adapter MUST preserve
those roles rather than flatten them into a generic signer.

## Profile And Implementation Evidence

An interpreter profile SHOULD be independently addressable from every
implementation of it. The profile may contain or reference:

- a specification, formal description, or pseudocode;
- parameter sets and encoding rules;
- security assumptions and intended purposes;
- conformance and negative test vectors;
- known limitations and incompatible versions;
- source code, portable bytecode, and platform binaries;
- build recipes, dependency locks, and reproducible-build instructions;
- expected resource use and confinement requirements.

Signed evidence about a profile or implementation may include:

- authorship and maintenance responsibility;
- review or audit findings;
- conformance results;
- reproducible-build provenance;
- interoperability observations;
- deployment experience;
- vulnerability, compromise, withdrawal, or supersession status;
- community acceptance for a stated purpose.

Attestations MUST refer to exact profile or artifact identifiers. A statement
about an algorithm name, package name, branch, or mutable download location is
insufficient to identify executable bytes.

Evidence about a profile does not automatically apply to every implementation.
Evidence about source does not automatically apply to a binary without an
appraised source-to-binary relationship.

## Appraisal Requirements

An Appraisal Policy for interpreters MUST be able to consider:

- interpreter kind and intended purpose;
- profile identity, version, and parameters;
- implementation identity and platform;
- authors, builders, reviewers, and independent witnesses;
- required conformance vectors and observed results;
- known vulnerabilities, revocations, and supersession claims;
- freshness and completeness of available status evidence;
- requested capabilities and confinement;
- whether the result will be advisory or authoritative for a subsequent
  decision.

Appraisal outcomes MUST distinguish:

- unsupported: the evaluator cannot process the profile;
- awaiting-context: required profiles, evidence, or status are missing;
- accepted-for-interpretation: the semantics may be relied upon for the stated
  purpose;
- accepted-for-execution: a particular implementation may run under a
  particular grant;
- rejected: policy has sufficient reason not to rely or execute;
- contested: decisive evidence or profile succession is unresolved.

Acceptance MUST be purpose-specific. An adapter accepted to display a
credential is not thereby accepted to authorize admission. A cryptographic
implementation accepted for test fixtures is not thereby accepted for
constitutional amendments.

## Execution Requirements

Execution is optional. A conforming Comms implementation MAY support Retain,
Transport, and externally supplied interpretation receipts without executing
untrusted artifacts.

Where execution is supported:

1. **Separate grant.** Every execution MUST be authorized separately from
   profile and implementation appraisal.
2. **Exact target.** The grant MUST identify the implementation artifact,
   interpreter profile, platform assumptions, and accepted input identifiers.
3. **Least authority.** The grant MUST enumerate filesystem, network, clock,
   randomness, environment, device, process, and secret access. Unmentioned
   capabilities are denied.
4. **Resource bounds.** The grant SHOULD constrain time, memory, output size,
   process count, and persistent storage.
5. **Deterministic mode.** Profiles claiming reproducibility MUST define all
   permitted nondeterminism and how it is recorded.
6. **No ambient identity.** An implementation MUST NOT receive a steward's
   signing key merely because it runs within that steward's tools.
7. **Output quarantine.** Derived artifacts and receipts MUST remain
   distinguishable from accepted evidence until separately appraised.
8. **Failure containment.** Crashes, timeouts, policy violations, and malformed
   outputs MUST NOT corrupt retained input artifacts or existing store state.

Comms need not provide a sandbox itself. It MUST expose enough manifest and
policy information for a host, operating system, virtual machine, capability
system, or external runner to enforce confinement.

## Receipts

An interpretation or execution receipt SHOULD bind:

- input artifact identifiers;
- interpreter profile identifier;
- implementation artifact identifier, if code executed;
- policy and execution-grant identifiers;
- platform or runner identity where relevant;
- declared capabilities actually granted;
- output artifact identifiers;
- structured result and uncertainty;
- start and completion observations;
- failures, omitted outputs, and resource-limit events;
- signer of the receipt.

The signer is the host, operator, runner, or another observer capable of
attesting to the event. The interpreter does not acquire personhood or a
private key merely by being executable.

A receipt proves only that its signer claims the stated operation occurred. It
does not make the implementation trustworthy or the output correct. Receipts
become evidence for later appraisal, reproducibility checks, and dispute.

## Cryptographic Profiles

Cryptographic algorithms are a special interpreter kind because they can
establish whether other evidence is authentic. Their profiles MUST identify:

- algorithm and exact parameter set;
- key, signature, digest, and encoding rules;
- domain-separation and canonicalization requirements;
- accepted purposes and prohibited combinations;
- security assumptions and known limitations;
- conformance and negative vectors;
- transition and retirement rules.

No algorithm is appraised as infallible. Relevant evidence concerns resistance
to known attacks, public analysis, implementation experience, conformance, and
suitability for a stated threat model.

The record being verified MUST NOT choose the Trustor's security policy merely
by naming an algorithm. Unknown algorithms MUST be retained as unsupported
rather than executed or accepted through automatic negotiation. Policies MUST
prevent algorithm confusion, downgrade, and substitution across profiles.

## Succession And Bootstrap

A new cryptographic verifier cannot authenticate the evidence authorizing
itself. Introduction of a new trust-establishing profile therefore crosses the
existing agreement's boundary and requires explicit succession.

A transition SHOULD:

1. identify the existing trusted profile and authority for amendment;
2. content-address the new profile, implementations, and vectors;
3. appraise the new profile using evidence verifiable under the existing
   profile;
4. define coexistence, conflict, rollback, and retirement rules;
5. use dual proofs during a transition period where practical;
6. preserve old records and verifiers needed to explain historical evidence.

Supersession is a claim, not a self-executing act. Competing algorithm-profile
successions MUST remain visible to the Trustor.

Classical, post-quantum, and hybrid profiles MAY coexist. Post-quantum support
SHOULD be selectable by purpose and threat horizon rather than imposed on every
resource-constrained deployment. Nodes unable to verify a post-quantum profile
SHOULD still be able to retain and transport its artifacts.

## Privacy And Selective Disclosure

An interpreter may understand only a deliberately disclosed projection of an
artifact. Profiles handling selective disclosure MUST preserve:

- what proof suite and disclosure mechanism were used;
- which statements were disclosed;
- which claims were proven without disclosure;
- audience, nonce, domain, or anti-replay bindings;
- correlation and privacy limitations;
- what undisclosed source material is not available to the evaluator.

An adapter MUST NOT reconstruct, infer, or claim possession of hidden
attributes merely to produce a fuller native representation. Opaque relay of a
presentation is valid even where a node cannot verify its proof.

## Licensing And Distribution

Interpreter appraisal is independent of software licensing. A Trustor may
appraise proprietary, copyleft, commercial, private, or permissively licensed
software.

Components incorporated into the upstream Comms distribution MUST remain
compatible with Comms' permissive, forkable, and redistributable character.
First-party adapters MAY target systems Comms cannot redistribute, provided
their own code and use comply with applicable terms. Third-party integrations
are not subject to an upstream protocol-level license test.

Licensing evidence MAY affect whether an implementation can be obtained,
redistributed, audited, reproduced, or executed in a deployment. It MUST NOT be
misrepresented as cryptographic or semantic validity.

## Non-Goals

This requirements document does not:

- define an interpreter manifest or receipt claim schema;
- mandate downloadable or executable code;
- select a sandbox technology;
- establish a universal software reputation score;
- make Vouch paths authorization chains;
- require every node to understand every carried artifact;
- require cryptographic agility in Attest 1.0;
- select a post-quantum algorithm;
- collapse foreign evidence into native claims.

## Initial Work

The first specification and prototype should:

1. define content-addressed interpreter-profile and implementation-manifest
   schemas;
2. define interpretation and execution receipt schemas;
3. implement Retain and Interpret without dynamic code loading;
4. implement an in-toto/SLSA provenance adapter as the first bounded external
   evidence adapter;
5. adapt Status List 2021 and Bitstring Status List through one status adapter
   family while preserving their version-specific semantics;
6. define a host-facing confinement manifest before permitting executable
   adapters;
7. create negative tests proving that retention, bundle inclusion, successful
   parsing, and Vouch acceptance do not authorize execution;
8. specify cryptographic-profile succession only after the ordinary adapter
   lifecycle is exercised.

The reference implementation should begin with statically linked or
out-of-process interpreters selected by explicit configuration. Dynamic
execution is a later capability, not the proof that this architecture works.
