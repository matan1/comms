# Plan: Transaction & Activity Visualization for the Community Simulator

Status: **Deferred** (agreed direction; implementation postponed)
Date drafted: 2026-05-29
Component: `community-sim/`

## Problem

It is hard to tell what individual stewards are actually *doing* each cycle.
The simulator advances ceremonies, attestations, allocation, and a goods market,
but the only spatial view (the "Trust Fabric" network) does not show real
activity.

Root cause found while investigating: **the edges drawn on the network are
fake.** In `drawNetwork`, connecting lines come from `randForPair(s.id, m.id)` —
a per-cycle hash of the two steward IDs gated by their trust product. They
re-randomize every cycle and correspond to no real event. The main
visualization is therefore not showing any actual relationship or transaction.

## Key insight: the data already exists

Every transaction is already recorded as an attestation in `state.attestations`,
each carrying a `cycle` number and participant IDs in `detail`. A transaction
visualization is therefore just a **faithful render of the audit graph**, which
is thematically on-point for a Comms ("auditable coordination") demo.

Attestations with both endpoints (drawable as edges):

- `endorsement/1` → `{by, target}` — a sponsor vouching for a candidate
- `objection/1` → `{by, target}` — a witness disputing a ceremony/claim
- `recognition/1` / `general-claim/1` → `{by, target}` — peer recognition/observation

Attestations attached to a single actor (drawable as node badges/pulses):

- `purchase-decision/1` → `{by, primary_good, spent, satisfaction}`
- `price-signal/1` → `{by, good, quote}`
- `allocation-return/1` → `{by, amount}`

Aggregate, non-pairwise records:

- `market-clearing/1` → `{mode, volume, unmet, prices}`
- `ceremony-record/1` → `{subject, sponsors (count), witnesses (count), objections (count)}`
  (note: stores **counts**, not sponsor/witness IDs)

## Important model constraint

**Trades are not bilateral.** In `runGoodsEconomy`, buyers purchase from an
anonymous pool (`good.supply - good.cleared`), not from a named seller. So there
is no "Ari sold grain to Bex" relationship in the model — only "Ari produced
grain" and "Bex bought grain." Economic activity is best shown as
steward↔good flows or a per-steward ledger, **not** as steward-to-steward trade
lines.

If true "who-traded-with-whom" lines are desired, the market must first be made
bilateral (match buyers to specific sellers). That is a model change that would
alter outcomes and trust dynamics, and is explicitly **out of scope** for this
plan unless separately approved.

## Recommended approach (agreed)

A split, because social and economic activity have different shapes:

### 1. Superimpose real social flows on the existing network

Replace the fake `randForPair` edges with real per-cycle edges built from the
attestation log:

- endorsement → blue
- objection → red
- recognition / general-claim → teal

Edges pulse on the current cycle and fade over ~2–3 cycles. Driven entirely by
`state.attestations` filtered to recent cycles. No layout change; fixes the view
in place where users already look.

### 2. Separate compact economic panel

Because trade is pooled/aggregate, present it as either (or both):

- a **bipartite stewards ↔ goods flow** (production into the market, purchases
  out of it, credits spent), and/or
- a **per-steward "this cycle" ledger** table: produced, bought, grant,
  sponsored, objected, returned — current cycle only, sortable.

The ledger is the lowest-effort, highest-legibility option and most directly
answers "what is everyone doing."

## Why not superimpose everything on one canvas

The network canvas is already spatially busy (nodes positioned on a ring,
colored by trust, sized by capability). Overlaying aggregate economic flows on
it would (a) crowd the view and (b) mislead, since pooled trade does not map to
point-to-point lines. Social = relational → belongs on the network; economic =
aggregate/per-actor → belongs in its own panel.

## Implementation sketch

1. **Edge extraction helper**: `recentSocialEdges(window = 3)` reads
   `state.attestations`, filters to `cycle >= state.cycle - window` and the
   three pairwise types, resolves `by`/`target` IDs to steward objects (need an
   `id → steward` lookup), and returns `{from, to, type, age}`.
2. **`drawNetwork` change**: remove the `randForPair` edge block; draw
   `recentSocialEdges()` with per-type color and `alpha = f(age)`.
3. **Node activity badges** (optional, phase 2): tally per-steward economic acts
   for the current cycle and render small markers (e.g. a coin for a purchase,
   an up-arrow for a price quote, a return glyph).
4. **Economic panel**: new `.panel` in `index.html`, new `drawEconomy()` /
   `renderLedger()` in `app.js`, styles in `styles.css`. Reads the same
   attestation log + `state.market`.
5. **Legend** updates for the new edge colors.
6. Keep everything static and deterministic; no new dependencies.

## Test plan (when implemented)

- `node --check community-sim/app.js`.
- Headless harness: confirm `recentSocialEdges` returns edges whose endpoints
  exist among active stewards, and that counts match attestations emitted that
  cycle.
- Browser smoke: edges appear/fade in step with ceremonies; economic panel
  updates each cycle; export still works.
- Verify a quiet cycle (no ceremony) shows no phantom social edges — the
  original bug.

## Open questions for the eventual implementation

- Animated (live pulsing/moving) vs. a clean per-cycle snapshot?
- Should the economic view be the bipartite flow, the ledger, or both?
- Do we ever want bilateral trade (the larger model change), or keep trade
  pooled and visualize it as flows only?
