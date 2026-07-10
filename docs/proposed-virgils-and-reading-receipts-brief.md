# Future Brief — Proposed Virgils, Sortition, and Reading Receipts

Status: research and implementation handoff after Session 12 (Sol). Depends on
the interpretive overlay described in
`archive-manifest-levels-implementation-brief.md`.

## Purpose

Interpretive overlays offer companionship. A proposed Virgil is one attributed
reader's route through an archive, never the archive's voice. Reading receipts
record the aperture through which history reached a reader without claiming
that opening produced understanding or agreement.

## Proposed Virgil

A Virgil is an overlay plus a named reading path. It must disclose:

- overlay attestation id and author;
- source manifest snapshot;
- exact ordered artifact/citation set;
- selection rule and intended purpose;
- declared omissions or scope;
- counterpoints deliberately included;
- whether it was chosen directly, by policy, or by sortition.

The TUI should say `proposed by <steward>` or `selected by sortition`, never
`recommended by the archive`.

## Sortition

Sortition counters popularity and recency becoming authority. It must remain
reproducible and resistant to after-the-fact steering.

Candidate `sortition/1` procedure:

1. Fix the manifest snapshot and purpose.
2. Fix and sort the eligible overlay ids before revealing the seed.
3. Record every eligibility/exclusion rule.
4. Obtain a seed from user choice, OS randomness with a signed receipt, or a
   commit/reveal ceremony when participants distrust unilateral selection.
5. Compute for every overlay:

   ```text
   H("comms.archive-virgil-sortition/1",
     snapshot || purpose || seed || overlay_id)
   ```

6. Select the lowest digest; publish the input set, seed, and result.

Fixing eligibility before seed revelation prevents an overlay author from
grinding new ids after seeing the draw. Sortition says only who accompanies
this reading, not who is best.

An optional second draw may select a counter-Virgil from overlays whose cited
set is not identical to the first. Any diversity rule must be explicit; do not
hide ranking inside the word `counterpoint`.

## Reading receipts

Receipts are ordinary host-gated archive artifacts, detached by default. Keep
tool observation separate from reader testimony.

Tool-observed event vocabulary:

```text
delivered | opened-through-comms | inspected | compared | quoted | cited
```

Reader-attested vocabulary:

```text
considered | relied-upon | rejected | disagreed | left-uncertain
```

The tool cannot observe manual file access and must say its event log is
incomplete. `opened` never implies read, understood, remembered, or influenced.

Candidate body:

```json
{
  "schema": "comms.archive-reading-receipt/1",
  "reader": "comms.steward:z...",
  "manifest_snapshot": "comms.manifest:z...",
  "presentation": {
    "level": "minimal",
    "custodian_line_b3": "..."
  },
  "virgil": {
    "overlay": "comms.attest:z...",
    "selection": "comms.attest:z..."
  },
  "events": [
    {
      "artifact": "comms.attest:z...",
      "action": "opened-through-comms",
      "at": "...",
      "delivery": "comms.attest:z..."
    }
  ],
  "reader_statements": [
    {
      "artifact": "comms.attest:z...",
      "disposition": "relied-upon",
      "reason": "..."
    }
  ],
  "limitations": ["Manual access is not observed."]
}
```

The custodian threshold line should have its own BLAKE3 in a future manifest
presentation field. It remains outside the custody snapshot, but a receipt can
then identify which changing History greeted the reader.

## Consent and granularity

- Receipt creation is optional.
- Tool events may be kept as an unsigned local draft until the reader reviews
  them.
- The reader chooses exact, coarse, or no receipt.
- A custodian may attest delivery but cannot attest the reader's reliance.
- A reader may omit reasons or detach the whole body.
- Receipt absence says nothing about whether material was accessed.

## TUI work

- `Begin receipt` binds a draft to the current manifest presentation.
- Opening through the TUI appends visible draft events.
- The receipt pane lets the reader remove events, coarsen them, and add only
  their own dispositions.
- `Seal receipt` previews exact bytes and signs under the reader key.
- Virgil selection displays eligibility, seed, and algorithm before opening
  the path.
- Exiting with an unsigned draft asks whether to retain locally, discard, or
  return without deciding. No auto-sign.

## Tests and acceptance

- Sortition is deterministic for fixed inputs and changes when any input does.
- Eligibility is frozen before the seed in commit/reveal mode.
- Grinding an ineligible post-seed overlay cannot change the result.
- Tool and reader event classes cannot be substituted in encoding.
- Receipts can identify a threshold presentation without changing snapshot id.
- Manual-access incompleteness is always represented.
- No receipt is created merely by launching the TUI.

