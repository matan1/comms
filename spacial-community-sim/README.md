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
- **Distance is paid for, not flagged.** Every spatial relationship runs
  through a cost field computed from the terrain the map shows (see below).
  Whether a villager makes the market trip, how much of the phase the walk
  eats, how far hearth gossip carries — all of it is geometry.

## The cost field

`world.js` rasterizes the terrain into a coarse grid (roads, woods, standing
crops) and exposes one general query:

```js
cost(operation, locationA, locationB, params)   // the general form
travelCost(a, b, { cart: true })                // convenience wrappers
messagingCost(a, b)
```

An **operation** (registered via `registerCostOp`) decides what each terrain
cell costs and how steeply roads discount the trip; **params** let the same
operation answer differently per traveler (a cart rides roads cheaply but
suffers in the woods — the hook exists, though nobody owns a cart yet).
A query is the cheaper of going straight (a terrain integral) or walking to a
road, riding it, and walking off. No pathfinding: it is a cost model, not a
physics engine, and it is RNG-free, so determinism survives. The grid is
rebuilt only when terrain changes (`rebuildCostField()`), which is the seam
the upcoming terrain editor will use.

## The day

Four phases, walked on the map: morning work at home → midday market (bilateral
witnessed trades) → afternoon commons (petitions and admission ceremonies) →
evening hearth gossip between neighboring homes.

Presence at the market and the commons falls off with the trip's cost, and
attendees who paid a long walk arrive with less of the phase left (fewer
trades, less gossip). Attendees are assigned spots in the venue *before*
interactions run, and witnesses and gossip partners are chosen from those
spots — the walking animation is a replay of consequences, not theater.

The **farmstead** cluster up the north-east road carries no special flag
anymore. It lags because it is far: the road discounts the trip but doesn't
erase it, so news reaches it late and its beliefs visibly trail the village
square. The same is true of any outlying house — including ones the village
builds for itself as it grows. The commons sits west of the square, so the
map develops two information basins (market-rich east, ceremony-rich west)
bridged by gossip.

Housing grows on demand: when the founding plots run out, a new plot is
surveyed and the cheapest commute wins, so the village creeps outward along
its roads.

## Things to try

- **Inject defector**, let them get admitted, then watch the village's view of
  them collapse over the following days while the farmstead still trusts them.
  Click a farmsteader: the inspector calls out where they disagree with the
  village mean ("hasn't heard the bad news").
- Click villagers on opposite sides of the village: the inspector now prices
  their trip to market. The far side of the ring really does miss more
  market days — and holds older news.
- Drop **Travel willingness** and **Gossip depth** to widen the lag; raise
  them to watch the community synchronize.
- Tighten **Sponsors required** / **Witness quorum** and watch admission slow.
- Watch **Fresh-news coverage** (how much of the last 3 days' attestations the
  average member holds) and **Belief spread** (how much members disagree about
  each other) move when a scandal lands.

## Headless harness

Logic and rendering are split across four classic scripts, loaded in order
and sharing scope — the same order a Node harness concatenates them in:

```
world.js    terrain, the cost field, the cost API, plot lifecycle
sim.js      villagers, attestations, beliefs, phases, gossip, metrics
render.js   canvas painting, pulses, walking animation
ui.js       DOM wiring, controls, inspector, frame loop
```

`seedState(p)`, `advanceDay(p)`, `nextPhase(p)`, and `normalizeParams(raw)`
run without a DOM (`hasDom` guards all DOM access). `harness-test.js` is the
validation scenario set used during this revision: determinism, emergent
farmstead attendance and coverage lag, housing growth under 500 days of
arrivals (the day-566 "everyone lives at plot zero" bug is fixed), the
defector divergence arc, and cost-API sanity. Run it with:

```sh
cat world.js sim.js render.js ui.js harness-test.js | node
```

`normalizeParams` still accepts the old `marketAttend` key as an alias for
`travelWill`, so existing harness scenarios keep running.

## Vouch research harness

The simulator now carries the original model's 16 adversary presets and an
informative Vouch profile alongside the legacy flat tally. Vouch counts
distinct direct interaction issuers, caps repetition, treats objections as
contestation, and lets buyers stop relying on a steward once two independent
failures reach their local store.

The browser controls expose the flat/Vouch appraisal choice and all 16
adversary behaviors. Select a behavior under **Adversary laboratory**, inject
it, and click villagers to compare their locally held evidence and Vouch
outcomes.

Movement is phase-budgeted: every trip completes within the current visual
phase at every pace, while residents staying home remain exactly at their home
instead of drifting. Interaction data is prepared at the phase transition but
kept hidden during travel. After everyone arrives, lines briefly fade in and
out before the next departure; a moving marker shows direction where the
relationship has one. With a villager selected, the display is limited to
interactions they observed and adds lighter evidence lines for attestations
that actually informed their own direct decisions.

Injected adversaries retain their preset type. Omniscient view gives each of
the 16 types a stable default color and persistent `name · type` label so their
survival, expulsion, and accumulation are visually traceable. Selecting a
villager hides those labels and colors because that ground truth is not part of
the villager's evidence.

Run the full comparison (25 deterministic seeds per preset, 120 days):

```sh
cat world.js sim.js render.js vouch-adversary-test.js | node
```

For a fast smoke run, set `VOUCH_SEEDS=3`. The harness reports harmful
post-admission deals, adversary admission, and honest-newcomer admission cost.

## Deliberate simplifications (and where this goes)

- One community; the farmstead is distance — now literally. The next tier
  adds neighboring settlements, couriers carrying **bundles**, schisms, and
  succession scenarios from Steward 1.0. `messagingCost` and the `cart`
  param are the hooks it will land on.
- Travel is a cost, not a path: villagers still walk through obstacles, they
  just pay for them. Pathfinding buys realism, not insight, and would cost
  the determinism the harness depends on.
- Beliefs use flat tallies; no endorser-weighted transitivity (Vouch territory).
- The economy is a thin pretext for generating witnessed commitments; v1's
  allocation/market depth was intentionally left behind here.
- Attestations are sim-level records, not spec-encoded envelopes — same
  illustrative (not normative) stance as v1.
