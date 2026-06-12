# Comms Village — spatial community simulator (prototype 2)

A reimagining of `community-sim/` around **literal space and partial
knowledge**. Static browser demo: no build, no dependencies, deterministic
under a seeded PRNG. Open `index.html` directly or serve:

```sh
python3 -m http.server 8080
```

## What it models

The previous simulator gave every steward a global trust scalar and an
omniscient ledger. This one takes the protocol at its word instead:

- **Every trust input is an attestation** — deal records, endorsements,
  objections, ceremony records. There is no global trust value anywhere.
- **Attestations are born at a place** and are initially known only to whoever
  was present (buyer, seller, a couple of bystanders; ceremony attendees).
- **Knowledge travels by presence and gossip.** Each villager keeps their own
  store (`knowledge` set + `feed` in learn order); co-present villagers and
  evening neighbors exchange a few recent items at a time.
- **Trust is viewer-relative**: a villager's opinion of another is a pure
  function of the attestations they hold, plus the community prior. Click any
  villager and the whole map recolors to *their* beliefs; villagers they hold
  no record of render gray as strangers.
- **Ceremonies count sponsors and objectors by each attendee's own beliefs**,
  and the ceremony record is known only to those who came. A defector can be
  admitted by people the bad news hasn't reached.

## The day

Four phases, walked on the map: morning work at home → midday market (bilateral
witnessed trades) → afternoon commons (petitions and admission ceremonies) →
evening hearth gossip between neighboring homes.

The **farmstead** cluster up the north-east road only makes the market trip
some days. It is the in-community stand-in for sneakernet latency: news reaches
it late, and its beliefs visibly lag the village square.

## Things to try

- **Inject defector**, let them get admitted, then watch the village's view of
  them collapse over the following days while the farmstead still trusts them.
  Click a farmsteader: the inspector calls out where they disagree with the
  village mean ("hasn't heard the bad news").
- Drop **Farmstead market trips** and **Gossip depth** to widen the lag;
  raise them to watch the community synchronize.
- Tighten **Sponsors required** / **Witness quorum** and watch admission slow.
- Watch **Fresh-news coverage** (how much of the last 3 days' attestations the
  average member holds) and **Belief spread** (how much members disagree about
  each other) move when a scandal lands.

## Headless harness

Same convention as v1: logic and rendering are split. `seedState(p)`,
`advanceDay(p)`, `nextPhase(p)`, and `normalizeParams(raw)` run without a DOM
(`hasDom` guards all DOM access), so a Node harness can concatenate `app.js`
with test code and drive whole scenarios. This was used to validate
determinism, admission pacing, gossip throughput, and the defector divergence
arc during development.

## Deliberate simplifications (and where this goes)

- One community; the farmstead is distance, not a separate steward. The next
  tier adds neighboring settlements, couriers carrying **bundles**, schisms,
  and succession scenarios from Steward 1.0.
- Beliefs use flat tallies; no endorser-weighted transitivity (Vouch territory).
- The economy is a thin pretext for generating witnessed commitments; v1's
  allocation/market depth was intentionally left behind here.
- Attestations are sim-level records, not spec-encoded envelopes — same
  illustrative (not normative) stance as v1.
