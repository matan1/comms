# Comms Vouch 1.0

Status: candidate specification. Vouch is Layer 4. A conforming Vouch result is
a policy-relative judgment, not a protocol truth.

## 1. Query and result

An evaluation query is:

```
subject    steward id or attestation id
purpose    exact UTF-8 capacity/action name
community  optional steward id
as_of      canonical RFC 3339 UTC timestamp
```

The evaluator returns one of `trusted`, `rejected`, `contested`, or
`awaiting-context`, plus the selected policy ID, store-view digest, counted and
ignored evidence, unresolved IDs, challenges, dispositions, paths, and applied
thresholds.

`store_view` is:

```
"comms.vouch.view:" + multibase(
  H("comms.vouch.view/1", canonical_cbor(sorted attestation ids))
)
```

## 2. `vouch-policy/1`

```
{
  t:          "vouch-policy/1"
  community:  <steward id>
  name:       <string>
  description:<string, optional>
  anchors:    [<steward id>, ...]
  purposes: [{
    purpose:               <string>
    positive_types:        [<claim type>, ...]
    negative_types:        [<claim type>, ...]
    min_positive_issuers:  <uint>
    min_negative_issuers:  <uint>
    min_endorsers:         <uint>
    issuer_cap:            <uint, >= 1>
    require_direct:        <uint: 0 | 1>
    propagation: {
      enabled:             <uint: 0 | 1>
      max_depth:           <uint, 1..4>
      min_paths:           <uint, >= 1>
    }
    positive:              <predicate, optional>
    negative:              <predicate, optional>
  }, ...]
}
```

Unknown purpose fields MUST be preserved. An evaluator MUST reject a policy
whose required fields or operators it does not understand. Purpose matching is
exact. The first matching purpose entry is used; duplicate purpose entries
make the policy malformed.

The optional predicates use this closed grammar:

```
{op: "all", of: [<predicate>, ...]}
{op: "any", of: [<predicate>, ...]}
{op: "not", of: <predicate>}
{op: "evidence-count", class: "positive"|"negative"|"endorsement", min: <uint>}
{op: "distinct-issuer-count", class: "positive"|"negative"|"endorsement", min: <uint>}
{op: "independent-path-count", min: <uint>}
{op: "unresolved-count", max: <uint>}
```

Unknown operators make a policy unsupported; evaluators MUST NOT guess. When
predicates are absent, the three `min_*` fields are the shorthand used by the
informative reference profile.

Evidence targets the query subject through a claim's `target`, `agent`,
`steward`, or `successor` field. `endorsement/1` additionally requires
`in_capacity == purpose`. `action-record/1` requires `action == purpose`;
outcomes `completed`, `success`, and `fulfilled` are positive, while `failed`,
`harm`, and `breach` are negative.

An attestation contributes only when all its signatures verify in the supplied
view. Community signatures additionally require their complete keyset chain.
Unverifiable or unresolved inputs are traced but not counted.

Distinct issuer counts, not attestation counts, satisfy thresholds. At most
`issuer_cap` records from one issuer contribute to an evidence class.
Endorsements are counted separately and cannot satisfy
`min_positive_issuers`.

## 3. Challenges and dispositions

An `objection/1` challenges its target attestation. It does not automatically
become negative reputation. If it targets decisive evidence and itself
verifies, the result is `contested` unless policy-specific evidence resolves
the challenge.

```
{
  t:       "vouch-disposition/1"
  target:  <attestation id>
  state:   "active" | "inactive" | "reaffirmed"
  reason:  <string, optional>
}
```

A disposition affects a target only when signed by an issuer of that target.
Disposition history uses `supersedes` refs. A unique latest head determines
state; multiple verified heads from the same predecessor are contested.
`inactive` excludes the target. `active` and `reaffirmed` include it.

## 4. Trust paths

Trust paths are a native but opt-in evaluator capability. With propagation
disabled, only direct eligible issuers count. With it enabled, an issuer is
eligible when at least `min_paths` cycle-free endorsement paths connect a
policy anchor to that issuer, each no longer than `max_depth` (hard maximum
four). Paths must be independently rooted or have distinct first hops.

Paths establish eligibility only. They never add weight, duplicate evidence,
or turn an endorsement path into direct behavioral evidence.

## 5. `vouch-judgment/1`

```
{
  t:          "vouch-judgment/1"
  subject:    <steward id or attestation id>
  purpose:    <string>
  community:  <steward id, optional>
  policy:     <vouch-policy/1 attestation id>
  as_of:      <timestamp>
  outcome:    "trusted" | "rejected" | "contested" | "awaiting-context"
  store_view: <digest above>
  engine:     <implementation/version string>
  evidence:   [<attestation id>, ...]
  unresolved: [<attestation id>, ...]
}
```

A judgment is a receipt: its signature proves who issued the receipt, not that
the computation or selected policy should be trusted. Receipts are optional.

## 6. Informative reference profile

The supplied profile requires direct positive interactions from distinct
counterparties plus distinct sponsors for admission, caps each issuer at one
contribution per class, requires multiple independent negative action records
for rejection, treats objections as challenges, and defaults propagation off.
Cross-community purposes may enable paths with depth two and two independent
paths.

`data/vouch-reference-policy.json` carries the profile as a deterministic,
personally signed attestation vector. It is an example that must be selected
explicitly, not a fallback policy.

## 7. Conformance

A reference implementation must pass golden cases for positive, negative,
contested, partial-context, withdrawal, disposition forks, issuer farming,
bounded paths, cycles, succession, and stable receipt bytes. Simulator
evaluation is empirical rather than protocol conformance.
