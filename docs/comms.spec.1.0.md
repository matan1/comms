Attest 1.0

What follows is a specification suitable for being implemented against. I've tried to keep it minimal while making sure ceremonies can be properly recorded and that the foundational claim types for early community life are present.
Overview

An attestation is a signed statement about something. The statement is the claim. The contextual surround is the frame. The signers are signatories. References to other attestations are refs. The whole thing is content-addressed.

Wire format is CBOR with deterministic encoding (RFC 8949 §4.2.1 core deterministic encoding). A JSON projection is defined for human consumption; the canonical hash is always computed over the CBOR form. Implementations MUST produce identical hashes from the canonical CBOR form regardless of how the document was authored.
Identifiers

Steward identifiers: comms.steward: followed by a multibase-encoded representation. For a single-key steward, this is the Ed25519 public key. For a multi-key steward (a community, a threshold group), this is the blake3 hash of the canonical keyset descriptor.

Attestation identifiers: comms.attest: followed by multibase blake3 hash of the canonical CBOR core (the document with signatures field removed).

Multibase prefix z (base58btc) is the default for both. Other multibase encodings MAY be used but z is what implementations MUST accept.
Envelope

The CBOR document is a map with the following top-level fields. Field names are short strings to keep the wire format compact; the JSON projection uses longer names shown in parentheses.

{
  v (version):        1
  t (type):           "comms.attestation/1"
  c (claim):          <claim object, see below>
  f (frame):          <frame object>
  r (refs):           [<ref object>, ...]    // may be empty array
  s (signatures):     [<signature object>, ...]
}

The core (used for hashing and signing) is the document with the s field omitted. The attestation identifier is comms.attest:z + multibase blake3 hash of the canonical CBOR core.
Claim

The claim is a map containing at minimum:

{
  t (type):           <string, e.g. "ceremony-record/1">
  ...type-specific fields...
}

Six claim types are defined for 1.0:

general-claim/1 — A statement about something, with optional content and supporting references.

{
  t:                  "general-claim/1"
  about:              <string or structured identifier of what the claim is about>
  kind:               <string: "observation" | "synthesis" | "prediction" | "testimony" | "translation" | "other">
  content:            {
    media_type:       <string, e.g. "text/markdown">
    body:             <bytes or string>     // inline if small
    body_hash:        <hash>                 // alternative to body, for large content stored externally
  }
  support:            [<attestation id>, ...]   // attestations supporting this claim
}

This is the general-purpose primitive. A scientific observation, a witness statement, a synthesis paper, a folk-knowledge record, a translation - all use this type with different kind values. The community context in the frame determines what conventions apply.

ceremony-record/1 — The record of a community gathering at which attestations were made.

{
  t:                  "ceremony-record/1"
  kind:               <string: "coming-into-community" | "renewal" | "rule-adoption" | "departure" | "key-rotation" | "other">
  community:          <steward id of the community>
  subject:            <steward id of the person/entity centered in the ceremony, may be omitted for community-only ceremonies>
  what_happened:      {
    narrative:        <string, human-readable account in the frame's primary language>
    elements:         [<element object>, ...]   // structured events that occurred
  }
  presence:           {
    in_body:          [<steward id>, ...]        // physically present
    remote:           [<steward id>, ...]        // present by other means
  }
  place:              {
    description:      <string>                   // required, human-readable
    coordinates:      {lat: <number>, lon: <number>}  // optional, may be omitted
  }
  artifacts:          [<artifact object>, ...]   // physical tokens, signed papers, etc.
  media:              [<media object>, ...]      // photos, recordings, etc.
}

Element objects describe what occurred during the ceremony in machine-readable form:

{
  kind:               <string: "key-generation" | "declaration" | "naming" | "presentation" | "reception" | "rule-acceptance" | "other">
  by:                 <steward id>                // who did this
  ...kind-specific fields...
}

For example, a key-generation element includes the generated public key fingerprint and a flag indicating witnesses verified it. A rule-acceptance element references the attestation id of the rule being accepted. A declaration element includes the text of what was declared.

Artifact objects describe physical tokens:

{
  description:        <string>
  custodian:          <steward id>                  // who keeps it
  media_hashes:       [<hash>, ...]                  // hashes of photos/scans, optional
}

Media objects describe digital records of the ceremony:

{
  media_type:         <string, e.g. "image/jpeg">
  hash:               <hash>
  description:        <string>
}

The actual media bytes are stored externally and referenced by hash. Implementations SHOULD support storing media alongside attestations in bundle formats; this is left to a future spec.

identity-binding/1 — A standalone identity binding attestation, used when a binding is recorded outside a ceremony (rare, but possible for, e.g., key rotations among already-bound members). For initial coming-into-community, the binding is part of the ceremony-record; identity-binding/1 is for special cases.

{
  t:                  "identity-binding/1"
  steward:            <steward id>                   // the identity being bound
  in_community:       <steward id>                   // the community context
  name_in_community:  <string>                       // optional
  supersedes:         <attestation id>               // optional, previous binding being replaced
  authority:          <attestation id>               // ceremony or rule that authorizes this binding
}

rule/1 — The document under which a community lives.

{
  t:                  "rule/1"
  community_name:     <string>
  community:          <steward id>
  document:           {
    media_type:       <string>
    body:             <bytes or string>
    body_hash:        <hash>                          // alternative for large rules
  }
  supersedes:         <attestation id>                // optional, previous version
  based_on:           <attestation id>                // optional, parent rule this derives from
  amendment_summary:  <string>                        // if supersedes, what changed
}

The rule attestation is signed by whoever has authority to set the rule. For a community's first rule, this is signed by the founding members. For subsequent rules, the prior rule defines the amendment procedure and the signers required.

endorsement/1 — A vouching for something.

{
  t:                  "endorsement/1"
  target:             <attestation id or steward id>
  in_capacity:        <string>                        // what this endorsement is about
  weight:             <string>                        // community-defined; common values: "primary", "secondary", "provisional"
  expires_at:         <timestamp>                     // optional
  rationale:          <string>                        // optional, human-readable
}

objection/1 — A registered disagreement with an attestation.

{
  t:                  "objection/1"
  target:             <attestation id>
  kind:               <string: "factual" | "procedural" | "ethical" | "scope" | "other">
  grounds:            <string>                        // human-readable
  evidence:           [<attestation id>, ...]         // supporting attestations
}

Frame

The frame provides contextual surround that applies to the claim. Fields:

{
  issued_at:          <ISO 8601 timestamp>            // signed assertion, not proof
  language:           <BCP 47 language tag>           // primary language of human-readable fields
  community:          <steward id>                    // optional, the community context
  occasion:           <string>                        // optional, human-readable context
  effective_period:   {
    from:             <timestamp>
    until:            <timestamp>                      // optional
  }                                                    // optional, for claims with bounded validity
}

Refs

References connect this attestation to others in the graph. Each ref has a role:

{
  role:               <string>
  id:                 <attestation id>
}

Standardized roles for 1.0:

    supersedes — this attestation replaces the referenced one
    supports — the referenced attestation provides evidence for this one
    responds-to — this attestation responds to the referenced one (objection, endorsement, follow-up)
    derived-from — the content of this attestation is derived from the referenced one
    context — the referenced attestation is part of the context needed to understand this one

Communities may introduce additional roles; implementations MUST preserve unknown roles when storing and forwarding attestations.
Signatures

The signatures field is an array of signature objects:

{
  by:                 <steward id>
  alg:                <string: "ed25519">              // others to be added in future versions
  signed_at:          <timestamp>
  role:               <string>                          // signer's role in this attestation
  signature:          <bytes>                           // raw signature over the canonical CBOR core's hash
}

Signature roles defined for 1.0: author, subject, witness, sponsor, community (signing on behalf of a community via threshold or designated authority).

The signature is computed as Ed25519(blake3(canonical_cbor_core)). Multi-key stewards (communities, threshold groups) produce signatures according to their keyset descriptor; the descriptor format and signing rules are specified in the Steward layer document (to be written).
Canonical form

CBOR encoding follows RFC 8949 §4.2.1 core deterministic encoding rules:

    Integers in shortest form
    Definite-length encoding for arrays, maps, strings, byte strings
    Map keys in bytewise lexicographic order of canonical encoding
    Floating-point in shortest preserving value
    No tags except those specified by this protocol

Implementations MUST be able to take any valid attestation and produce its canonical form. The canonical form is what gets hashed and signed.
Validation

An attestation is well-formed if:

    All required fields are present
    All identifiers parse correctly
    All signatures verify against the canonical core's hash
    All referenced attestations are reachable (in this context or in a referenced bundle)

An attestation is trusted by a viewer to the extent that:

    Its signatures are by stewards the viewer recognizes
    Its claims are of types the viewer's tools can interpret
    Its frame matches contexts the viewer considers valid
    Its references resolve to attestations the viewer also trusts

The protocol provides well-formedness checking. Trust is not a protocol property; trust is a property of the viewing community's practices, supported by the Vouch layer (to be specified separately).
Sneakernet bundle format

A bundle is a CBOR array of attestations plus an optional media-blob section:

{
  v:                  1
  t:                  "comms.bundle/1"
  attestations:       [<attestation>, ...]
  media:              {<hash>: <bytes>, ...}    // optional, content addressed by hash
  manifest:           {                          // optional
    created_at:       <timestamp>
    created_by:       <steward id>
    description:      <string>
  }
}

A bundle can be written to any medium - USB stick, optical disc, paper QR codes for small bundles. Implementations MUST accept bundles up to at least 100 MiB; recommended support extends to multi-gigabyte bundles for archival purposes.
Version evolution

Protocol version 1 is what's described here. Version 2 and beyond add fields and claim types; they MUST NOT change the canonical form rules or the validation rules in ways that would invalidate version 1 attestations. Implementations encountering unknown claim types or unknown ref roles MUST preserve them when storing and forwarding, even if they cannot interpret them.
What's deliberately not in 1.0

Revocation. A signer cannot, in 1.0, cryptographically revoke a signature they made. Practical revocation works through superseding attestations and through the trust layer (Vouch) marking attestations as no longer endorsed. This is intentional - cryptographic revocation is a complex and contentious topic, and 1.0 is small enough to make superseding sufficient. Version 2 may add revocation.

Encryption. Attestations are public by default. Encrypted payloads can be referenced by hash and stored with whatever access control the storing party chooses; the encryption scheme is not part of 1.0.

Time-stamping beyond signed assertions. There's no notion of a trusted time authority. Communities that need verified time can attest to events from external time sources, and these attestations are themselves first-class.

Network protocols for fetching attestations. Implementations may use whatever - HTTP, IPFS, sneakernet, email attachments. The protocol describes the documents, not how they move.


