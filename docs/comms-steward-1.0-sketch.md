# Steward 1.0 — Multi-Key Stewards, Rotation, and Succession (Sketch)

Status: draft sketch for review. Companion artifacts: `steward_vectors.py` (reference implementation — every rule below is executable) and `steward-test-vectors.json` (golden vectors). Depends on Attest 1.0 as amended by A1.

Design objectives, as ratified: unblock community signing end to end; flat n-of-m by counting, not cryptography; genesis-anchored identity; honest treatment of rotation and of broken chains; sneakernet-complete verification; protocol/policy split preserved. Non-objectives: nested groups, weighted votes, delegation, key recovery, cryptographic revocation, membership privacy.

---

## 1. Keyset descriptor

A keyset descriptor is the minimal object answering one question: *what counts as a valid signature by this steward.*

```
{
  v:          1
  members:    [ { key: <bytes, Ed25519 public key> }, ... ]
  threshold:  <int, 1 <= threshold <= len(members)>
}
```

Members MUST be sorted by bytewise comparison of `key` and MUST be unique. Member entries contain only keys: names, roles, and the binding of keys to people are ceremony-record and identity-binding territory, deliberately outside the descriptor. v1 members MUST be single Ed25519 keys — a community cannot (yet) be a member of a community.

## 2. Identity: genesis anchoring

A multi-key steward's identifier is derived from its **genesis** descriptor and never changes:

```
"comms.steward:" + multibase( H("comms.keyset/1", canonical_cbor(genesis_descriptor)) )
```

Rotation replaces the *current* descriptor but not the identifier. The alternative (ID = hash of current keyset) was rejected: it renames the community on every rotation and pushes continuity entirely onto supersession claims, which per A1.5 carry no inherent authority. Genesis anchoring keeps the name stable for as long as the chain (§4) is intact — a community survives replacing every founding key without becoming a stranger, provided each replacement happens one signed step at a time.

## 3. The `keyset/1` claim and community signatures

### 3.1 keyset/1

Descriptors travel as ordinary attestations, so the existing store, refs, and bundle machinery carry them for free:

```
{
  t:           "keyset/1"
  community:   <steward id>
  descriptor:  <keyset descriptor object>
}
```

### 3.2 Community signatures: `ed25519-set/1`

A community signs with a single signature object in the attestation's `s` array:

```
{
  by:         <community steward id>
  alg:        "ed25519-set/1"
  signed_at:  <timestamp>
  role:       <string>
  keyset:     <attestation id of the keyset/1 link this signature claims validity under>
  signature:  <bytes: canonical CBOR array of inner signatures>
}
```

Each inner signature is `{ k: <member Ed25519 public key>, s: <pure Ed25519 signature> }`; the array MUST be sorted by `k` with no duplicate keys. Every member signs the same payload — the A1.3 payload extended with the `keyset` field:

```
payload = canonical_cbor({ t: "comms.sig/1", core: <core hash>, by, alg, role, signed_at, keyset })
s = Ed25519_sign(member_sk, payload)
```

This is deliberately *threshold by counting*: m independent pure-Ed25519 signatures, verified one by one, with validity defined as **valid signatures from at least `threshold` distinct keys listed in the referenced descriptor**. No FROST, no aggregation, no signing ceremony — members sign asynchronously with whatever boring key they already hold (the Amendment-A1 decision propagated upward), and signatures can be collected over sneakernet across days. Aggregate schemes can arrive later as new `alg` values without disturbing anything.

Verification rules (executable in `verify_set_signature`):

- Inner signatures whose `k` is **not** in the referenced descriptor are **ignored**, not fatal — a hostile relay can pad the array but cannot poison it, and tolerant counting suits asynchronous collection.
- A **forged** signature from a key that *is* in the descriptor is fatal — that is evidence of tampering, not noise.
- Duplicate keys are fatal.
- The `keyset` field is inside the signed payload, so which chain link a signature claims validity under is authenticated and cannot be re-pointed after the fact (same class of fix as A1.3's role binding).

## 4. The chain: rotation

A rotation publishes a new `keyset/1` attestation that:

1. carries exactly one `supersedes` ref to the current keyset attestation, and
2. carries an `ed25519-set/1` signature by the community whose `keyset` field names the **predecessor** and which meets the **predecessor's** threshold.

The new keys do not authorize themselves; the keyset as it *was* authorizes the keyset as it *will be*. Ship of Theseus, one signed plank at a time.

**Genesis** is the chain's base case: a `keyset/1` attestation with no `supersedes` ref, whose descriptor hashes to the community ID (self-certifying), and which carries a community signature whose `keyset` field references *its own* attestation ID, meeting its own threshold. The self-signature is possible without circularity because signatures live outside the core, and it is required because it proves the founders actually possess the listed keys — you cannot found a community out of other people's public keys.

**Full verification** of any community-signed attestation (`verify_community_attestation`): take the signature's `keyset` link; walk `supersedes` refs back to genesis, checking at each link that the predecessor's threshold authorized it; check the genesis hash against the community ID; then verify the end signature against the link's descriptor. Inputs: the attestation, the chain, math. No registry, no resolver, no liveness assumption — the whole chain fits in a bundle.

**Forks.** Two valid successors to the same link are both cryptographically impeccable, and the protocol MUST NOT adjudicate between them — a fork is a schism, which is a community event, not an encoding error. Implementations MUST surface forks to the viewer rather than silently picking a branch. The trust layer, objections, and neighboring communities' endorsements are where schisms get resolved, exactly as they always have been.

## 5. Succession: when the chain breaks

The chain handles every *planned* change. It cannot handle key loss below threshold or destruction of the history itself, and this spec says so plainly rather than papering over it: **once the chain breaks, cryptographic continuity is unrecoverable by anyone, by definition** — any mechanism that could restore it without the old keys is the impostor's mechanism too. What follows is not recovery; it is *witnessed re-founding*.

### 5.1 succession/1

```
{
  t:                 "succession/1"
  predecessor:       <steward id of the broken community>
  successor:         <steward id of the new, freshly-genesised community>
  account:           <string: what happened, human-readable>
  continuity_basis:  <attestation id, optional: the predecessor rule containing a continuity clause>
}
```

with refs `successor-of` → the last known keyset link of the predecessor, and `context` → supporting history. Signed by the successor community (`ed25519-set/1` under its own new chain).

A succession claim has **no inherent authority** — this is A1.5's supersession principle applied to identities, and succession squatting on dead communities is the expected attack. Its weight comes entirely from attached evidence, for which the existing machinery already has slots:

- **Survivor testimony.** Old member keys below threshold are useless as *authority* but priceless as *evidence*: their plain-Ed25519 `witness` signatures on the succession attestation itself are the standard form of "I was there, this is the same village." The vectors demonstrate the mixed-algorithm attestation: one `ed25519-set/1` signature by the successor, plus personal `ed25519` witness signatures from a surviving current member and a long-departed founder whose key survived.
- **Neighbor testimony.** `endorsement/1` from communities that knew the predecessor.
- **The re-founding ceremony.** A `ceremony-record/1` with presence in body.

### 5.2 Continuity clauses: the will

A healthy community SHOULD pre-commit, in its `rule/1`, to the terms under which its succession may be recognized — e.g. *"succession requires witness signatures from at least two founding members and endorsement by the Eastbrook community."* Because the rule was signed by the fully valid keyset while it could still speak with authority, a later succession claim is not an after-the-fact plea evaluated from nothing; it is the execution of the community's own will, and `continuity_basis` points viewers to it. The vocabulary for machine-readable continuity parameters belongs to the Vouch layer (alongside the `vouch.*` family); this spec only fixes the hook.

Secret-sharing descriptor history across members' devices, naming guardian communities, escrow practices — all genuinely good ideas, all deliberately **community practice**, not protocol. The split holds.

## 6. Privacy consideration

Descriptors are public and enumerate exactly who can sign for a community — in some threat models, a target list. v1 accepts this openly rather than half-solving it; blinded membership is a possible v2 and a real research problem. Communities under threat can mitigate today by using keys not linked to legal identities and by keeping name↔key bindings in ceremonies they distribute narrowly.

## 7. Interaction with validation layers (A1.4)

Chain verification extends layer 3 (resolvable): a community signature is *verified* only relative to a resolvable chain. An attestation whose set signature is internally consistent but whose chain is absent from the viewer's context is structurally valid and awaiting context, not malformed — partial graphs remain the sneakernet norm. Whether a fully verified chain represents the community you *mean* remains, as ever, layer 4.

## 8. Test vectors

`steward-test-vectors.json`, generated and self-verified by `steward_vectors.py`:

- K0 genesis (2-of-3), self-signed; community ID derivation
- `rule/1` community-signed under K0 (containing a continuity clause)
- K1 rotation (one founder out, one member in), authorized by K0's threshold
- a rule amendment signed under K1, verifying through the full chain
- a successor genesis and a `succession/1` claim carrying the successor's community signature plus two personal witness signatures
- negatives: sub-threshold signatures fail; a departed member's key is ignored and cannot prop up threshold; a keyset claiming to supersede K0 without predecessor authorization fails chain verification (the hostile-takeover case)

A second implementation conforms only if it reproduces all hashes, identifiers, and payloads byte-for-byte and rejects all three negatives.
