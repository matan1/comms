// Comms Village — spatial community simulator (prototype 2).
//
// Reimagines community-sim around literal space and partial knowledge:
//   - the village is a map; villagers walk between home, market, and commons
//   - every trust input is an attestation, born at a place, initially known
//     only to whoever was present
//   - knowledge spreads by co-presence and evening gossip, so every villager
//     holds their own store and their own beliefs — trust is viewer-relative,
//     exactly as the protocol insists (well-formed != trusted)
//   - ceremonies count sponsors and objectors by each attendee's OWN beliefs;
//     a defector can be admitted by people the bad news has not reached yet
//
// Conventions carried over from community-sim v1: no dependencies, plain
// functions over a module-level `state`, a seeded PRNG so a scenario is
// deterministic, and a strict split between headless logic (seedState /
// advanceDay / runPhaseLogic) and rendering, so an Auto-Tune style harness
// can still drive the sim without a DOM.

"use strict";

// --- Static tables -----------------------------------------------------------

const names = [
  "Wren", "Sage", "Alder", "Briar", "Rook", "Tansy", "Orin", "Maren",
  "Hollis", "Petra", "Cassian", "Ida", "Tobin", "Lark", "Edda", "Soren",
  "Mira", "Joss", "Fenn", "Una", "Calder", "Niva", "Ember", "Roan",
  "Tilde", "Vesper", "Ondine", "Garron", "Liesl", "Marek", "Sunniva",
  "Corwin", "Hesper", "Dagny", "Ilias", "Ferra", "Oswin", "Prue"
];

const goodsTable = [
  { id: "grain", color: "#c9a24b" },
  { id: "timber", color: "#8a6a45" },
  { id: "cloth", color: "#7a6fa0" },
  { id: "tools", color: "#5f7d8c" },
  { id: "pots", color: "#a96e4f" }
];

const attColors = {
  "ceremony-record/1": "#2e6f9e",
  "endorsement/1": "#2f7d66",
  "objection/1": "#b65345",
  "deal-record/1": "#b87815"
};

const PHASES = [
  { key: "morning", label: "Morning" },
  { key: "market", label: "Midday market" },
  { key: "commons", label: "Commons" },
  { key: "evening", label: "Evening" }
];

// --- DOM handles (absent under a headless harness) ---------------------------

const hasDom = typeof document !== "undefined" && typeof window !== "undefined";

function byId(id) {
  return hasDom ? document.getElementById(id) : null;
}

const controls = hasDom ? {
  population: byId("population"),
  farmShare: byId("farmShare"),
  trust: byId("trust"),
  gossipRadius: byId("gossipRadius"),
  gossipDepth: byId("gossipDepth"),
  marketAttend: byId("marketAttend"),
  sponsors: byId("sponsors"),
  witnessQuorum: byId("witnessQuorum"),
  objectionRate: byId("objectionRate"),
  arrivalRate: byId("arrivalRate"),
  speed: byId("speed")
} : null;

// --- Parameters ---------------------------------------------------------------

function rawFromControls() {
  const raw = {};
  for (const key of Object.keys(controls)) {
    raw[key] = Number(controls[key].value);
  }
  return raw;
}

// Normalizes a raw control reading (or a hand-built object) into simulation
// units, so parameter sets can be constructed off-DOM.
function normalizeParams(raw) {
  return {
    population: Math.round(raw.population ?? 18),
    farmShare: (raw.farmShare ?? 25) / 100,
    trust: (raw.trust ?? 55) / 100,
    gossipRadius: (raw.gossipRadius ?? 12) / 100,   // world units (map is 1x1)
    gossipDepth: Math.round(raw.gossipDepth ?? 4),
    marketAttend: (raw.marketAttend ?? 25) / 100,
    sponsors: Math.round(raw.sponsors ?? 2),
    witnessQuorum: Math.round(raw.witnessQuorum ?? 5),
    objectionRate: (raw.objectionRate ?? 40) / 100,
    arrivalRate: (raw.arrivalRate ?? 35) / 100,
    speed: raw.speed ?? 5
  };
}

function params() {
  return normalizeParams(hasDom ? rawFromControls() : {});
}

// --- State ---------------------------------------------------------------------

let state;
let world;                 // map layout; rebuilt on reset, never mid-run
let running = false;
let phaseClock = 0;
let selectedId = null;     // selected villager == current viewpoint
let pulses = [];           // transient visual events (attestation births, gossip)
let lastFrame = 0;
let fallbackSeed = 4231;

function rand() {
  const seed = state ? state.seed++ : fallbackSeed++;
  const x = Math.sin(seed) * 10000;
  return x - Math.floor(x);
}

// Separate PRNG for map layout so terrain doesn't consume simulation entropy.
function makeRng(seed) {
  let s = seed;
  return () => {
    s = (s * 1664525 + 1013904223) % 4294967296;
    return s / 4294967296;
  };
}

function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }
function lerp(a, b, t) { return a + (b - a) * t; }
function dist(a, b) { return Math.hypot(a.x - b.x, a.y - b.y); }
function pick(items) {
  return items.length ? items[Math.floor(rand() * items.length) % items.length] : null;
}

// Weighted choice. Buyers prefer well-stocked stalls, which has a pointed
// side effect: a defector who keeps their goods while shorting deliveries
// becomes the best-stocked stall in the square — prominence is what exposes
// them. Sparse uniform picking made the scandal signal too slow to read.
function weightedPick(items, weightFn) {
  if (!items.length) return null;
  let total = 0;
  for (const item of items) total += Math.max(0.01, weightFn(item));
  let roll = rand() * total;
  for (const item of items) {
    roll -= Math.max(0.01, weightFn(item));
    if (roll <= 0) return item;
  }
  return items[items.length - 1];
}

// --- World layout ----------------------------------------------------------------
// Normalized 1x1 map. The village proper sits center-left; an outlying
// farmstead cluster sits up the north-east road. The farmstead is the cheap
// way to make gossip latency visible inside a single community: news reaches
// it a day or two late, and ceremonies it skips are ceremonies it never hears
// about firsthand.

function buildWorld(p) {
  const rng = makeRng(20260611);
  const center = { x: 0.40, y: 0.56 };
  const commons = { x: 0.305, y: 0.50, r: 0.052 };
  const market = { x: 0.485, y: 0.565, r: 0.058 };
  const farmstead = { x: 0.815, y: 0.205, r: 0.07 };
  const camp = { x: 0.265, y: 0.715 };          // where newcomers wait

  const homes = [];
  const count = 40;                              // plots; villagers claim them
  for (let i = 0; i < count; i += 1) {
    const angle = (Math.PI * 2 * i) / count + rng() * 0.12;
    // Skip the sector the market and commons occupy.
    const radius = 0.135 + rng() * 0.10;
    const x = center.x + Math.cos(angle) * radius * 1.25;
    const y = center.y + Math.sin(angle) * radius;
    if (dist({ x, y }, commons) < commons.r + 0.04) continue;
    if (dist({ x, y }, market) < market.r + 0.04) continue;
    homes.push({ x, y, claimed: false });
  }

  const farmHomes = [];
  for (let i = 0; i < 8; i += 1) {
    const a = rng() * Math.PI * 2;
    farmHomes.push({
      x: farmstead.x + Math.cos(a) * (0.02 + rng() * 0.045),
      y: farmstead.y + Math.sin(a) * (0.015 + rng() * 0.04),
      claimed: false
    });
  }

  const trees = [];
  for (let i = 0; i < 64; i += 1) {
    const x = rng();
    const y = rng();
    const pt = { x, y };
    if (dist(pt, center) < 0.27 || dist(pt, farmstead) < 0.12) continue;
    if (dist(pt, camp) < 0.06) continue;
    trees.push({ x, y, r: 0.006 + rng() * 0.008 });
  }

  const fields = [
    { x: 0.60, y: 0.80, w: 0.14, h: 0.085, angle: -0.12 },
    { x: 0.13, y: 0.30, w: 0.12, h: 0.075, angle: 0.18 },
    { x: 0.74, y: 0.33, w: 0.115, h: 0.07, angle: 0.32 },
    { x: 0.90, y: 0.12, w: 0.10, h: 0.06, angle: 0.05 }
  ];

  // Roads: village -> farmstead, and the south road newcomers arrive by.
  const roads = [
    [{ x: 0.46, y: 0.50 }, { x: 0.58, y: 0.40 }, { x: 0.70, y: 0.30 }, farmstead],
    [{ x: 0.36, y: 0.62 }, { x: 0.30, y: 0.73 }, { x: 0.22, y: 0.86 }, { x: 0.14, y: 0.99 }],
    [{ x: 0.46, y: 0.58 }, { x: 0.40, y: 0.55 }, { x: 0.355, y: 0.52 }]
  ];

  const stalls = [];
  for (let i = 0; i < goodsTable.length; i += 1) {
    const a = (Math.PI * 2 * i) / goodsTable.length - 0.5;
    stalls.push({
      x: market.x + Math.cos(a) * market.r * 0.72,
      y: market.y + Math.sin(a) * market.r * 0.72,
      good: goodsTable[i]
    });
  }

  return { center, commons, market, farmstead, camp, homes, farmHomes, trees, fields, roads, stalls };
}

function claimHome(pool) {
  const free = pool.filter((h) => !h.claimed);
  const home = free.length ? free[Math.floor(rand() * free.length)] : pool[0];
  home.claimed = true;
  return { x: home.x, y: home.y };
}

// --- Villagers -----------------------------------------------------------------

function makeVillager(index, opts = {}) {
  const farm = opts.farmstead === true;
  const home = opts.home || claimHome(farm ? world.farmHomes : world.homes);
  return {
    id: `comms.steward:z${String(index + 1).padStart(3, "0")}`,
    label: names[index % names.length],
    member: opts.member !== false,
    archetype: opts.archetype || "honest",
    farmstead: farm,
    specialty: goodsTable[index % goodsTable.length],
    capability: clamp(0.45 + rand() * 0.7, 0.2, 1.15),
    stock: 1 + rand(),
    home,
    pos: opts.pos ? { ...opts.pos } : { ...home },
    target: { ...home },
    wander: rand() * 1000,
    arrivedDay: state ? state.day : 0,
    joinedDay: opts.member !== false ? 0 : null,
    sponsors: [],
    atMarket: false,
    atCommons: false,
    // The villager's whole epistemic world: which attestations they hold
    // (feed preserves learn order so gossip can share recent news first) and
    // the per-subject tallies those attestations produce.
    knowledge: new Set(),
    feed: [],
    beliefs: new Map()
  };
}

// --- Attestations & beliefs ------------------------------------------------------
// Every trust input is an attestation. A villager's opinion of another is a
// pure function of the attestations they have actually received, plus the
// community prior. There is no global trust scalar anywhere in this file.

function learn(villager, idx) {
  if (villager.knowledge.has(idx)) return false;
  villager.knowledge.add(idx);
  villager.feed.push(idx);
  const att = state.attestations[idx];
  const subject = att.target;
  if (!subject) return true;
  let entry = villager.beliefs.get(subject);
  if (!entry) {
    entry = { pos: 0, neg: 0 };
    villager.beliefs.set(subject, entry);
  }
  if (att.type === "deal-record/1") {
    if (att.detail.outcome === "completed") entry.pos += 1; else entry.neg += 1;
  } else if (att.type === "endorsement/1") {
    entry.pos += 1;
  } else if (att.type === "objection/1") {
    entry.neg += 1.25;
  } else if (att.type === "ceremony-record/1" && att.detail.committed) {
    entry.pos += 2;
  }
  return true;
}

// Born at a place, known only to those present.
function addAttestation(fields, knowers) {
  const idx = state.attestations.length;
  state.attestations.push({
    idx,
    id: `comms.attest:z${String(state.nextId++).padStart(5, "0")}`,
    day: state.day,
    phase: state.phase,
    ...fields
  });
  for (const v of knowers) learn(v, idx);
  if (fields.at) {
    pulses.push({ kind: "attest", x: fields.at.x, y: fields.at.y, t: 0, color: attColors[fields.type] || "#637074" });
  }
  return idx;
}

function perceivedTrust(viewer, subjectId) {
  const subject = state.byId.get(subjectId);
  if (!subject) return 0;
  const b = viewer.beliefs.get(subjectId) || { pos: 0, neg: 0 };
  const prior = state.prior * (subject.member ? 1 : 0.72);
  const w = 6;
  return clamp((b.pos + prior * w) / (b.pos + b.neg + w), 0.02, 0.99);
}

function isStrangerTo(viewer, subjectId) {
  return viewer.id !== subjectId && !viewer.beliefs.has(subjectId);
}

// --- Seeding -------------------------------------------------------------------

function seedState(p) {
  world = buildWorld(p);
  state = {
    day: 1,
    phase: 0,
    seed: 2741 + p.population * 13 + Math.round(p.trust * 100),
    prior: p.trust,
    villagers: [],
    byId: new Map(),
    attestations: [],
    events: [],
    nextId: 1,
    nextVillager: 0,
    cached: { coverage: 0, spread: 0, mean: new Map() }
  };
  pulses = [];
  selectedId = null;

  const farmCount = Math.round(p.population * p.farmShare);
  for (let i = 0; i < p.population; i += 1) {
    const v = makeVillager(state.nextVillager++, { farmstead: i < farmCount });
    state.villagers.push(v);
    state.byId.set(v.id, v);
  }

  // A founding history: each villager arrives knowing a few true things about
  // their nearest neighbors, so day 1 isn't a village of mutual strangers.
  for (const v of state.villagers) {
    const neighbors = [...state.villagers]
      .filter((o) => o !== v)
      .sort((a, b) => dist(v.home, a.home) - dist(v.home, b.home))
      .slice(0, 4);
    for (const n of neighbors) {
      addAttestation({
        type: "deal-record/1",
        by: v.id,
        target: n.id,
        detail: { outcome: rand() < 0.85 ? "completed" : "failed", good: n.specialty.id, founding: true },
        at: null
      }, [v, n]);
    }
  }

  logEvent("rule/1", "Founding rule adopted", "Ceremony quorum, sponsor rule, and the gossip habits of the village are set.");
  runPhaseLogic(p, 0);
  assignTargets(p);
  refreshCaches();
}

function reset() {
  seedState(params());
  snapPositions();
  phaseClock = 0;
  renderStatic();
}

// --- Day structure ----------------------------------------------------------------
// One day = four phases. Logic for a phase runs once, at the moment the phase
// begins; rendering then animates villagers toward that phase's stations.
// advanceDay() runs a whole day headlessly — the harness entry point.

function nextPhase(p) {
  state.phase += 1;
  if (state.phase >= PHASES.length) {
    state.phase = 0;
    state.day += 1;
  }
  runPhaseLogic(p, state.phase);
  assignTargets(p);
  refreshCaches();
}

function advanceDay(p) {
  const remaining = PHASES.length - state.phase;
  for (let i = 0; i < remaining; i += 1) nextPhase(p);
}

function stepDay() {
  const p = params();
  do { nextPhase(p); } while (state.phase !== 0);
  snapPositions();
  renderStatic();
}

function runPhaseLogic(p, phase) {
  if (phase === 0) phaseMorning(p);
  else if (phase === 1) phaseMarket(p);
  else if (phase === 2) phaseCommons(p);
  else phaseEvening(p);
}

// Morning: production at home. Capability sets output; defectors produce
// normally — their cheat is in delivery, not effort.
function phaseMorning(p) {
  for (const v of members()) {
    v.stock = Math.min(4, v.stock + v.capability * (0.7 + rand() * 0.5));
  }
}

// Midday: whoever comes to the square trades face to face. Deals are bilateral
// and witnessed — buyer, seller, and a couple of bystanders learn the record
// at the moment it is made. Farmstead folk only make the trip some days,
// which is exactly how their knowledge falls behind.
function phaseMarket(p) {
  for (const v of state.villagers) {
    v.atCommons = false;
    v.atMarket = v.member
      ? (v.farmstead ? rand() < p.marketAttend : rand() < 0.86)
      : rand() < 0.7; // newcomers hang around the square making themselves useful
  }
  const attendees = state.villagers.filter((v) => v.atMarket);
  const sellers = attendees.filter((v) => v.member);

  for (const buyer of sellers) {
    const options = sellers.filter((s) => s !== buyer && s.specialty !== buyer.specialty && s.stock > 0.5);
    const seller = weightedPick(options, (s) => s.stock);
    if (!seller) continue;
    const cheat = seller.archetype === "defector" && seller.member;
    const failed = cheat ? rand() < 0.55 : rand() < 0.07;
    seller.stock = Math.max(0, seller.stock - 0.8);
    const witnesses = nearestOthers(attendees, buyer, 2);
    addAttestation({
      type: "deal-record/1",
      by: buyer.id,
      target: seller.id,
      detail: { outcome: failed ? "failed" : "completed", good: seller.specialty.id },
      at: world.market
    }, [buyer, seller, ...witnesses]);
    if (failed) {
      logEvent("deal-record/1", `${seller.label} shorted a delivery`, `${buyer.label} recorded the broken pledge at the market.`);
      if (rand() < 0.35 + p.objectionRate * 0.5) {
        addAttestation({
          type: "objection/1",
          by: buyer.id,
          target: seller.id,
          detail: { kind: "procedural", grounds: "delivery shortfall" },
          at: world.market
        }, [buyer, ...witnesses]);
      }
    }
  }

  // Newcomers earn their first reputations by helping out around the stalls.
  for (const cand of attendees.filter((v) => !v.member)) {
    if (rand() < 0.75) {
      const helped = pick(sellers);
      if (!helped) continue;
      const witnesses = nearestOthers(attendees, helped, 2);
      addAttestation({
        type: "deal-record/1",
        by: helped.id,
        target: cand.id,
        detail: { outcome: rand() < 0.92 ? "completed" : "failed", good: "labor" },
        at: world.market
      }, [helped, cand, ...witnesses]);
    }
  }

  gossipAmong(attendees, p);
}

// Afternoon: if a newcomer has waited long enough, the village gathers at the
// commons. Sponsorship and objection are judged from EACH attendee's own
// beliefs — the record of the ceremony is then known only to those who came.
function phaseCommons(p) {
  for (const v of state.villagers) v.atMarket = false;

  const waiting = state.villagers
    .filter((v) => !v.member
      && state.day - v.arrivedDay >= 2
      && state.day - (v.lastPetition || 0) >= 2)
    .sort((a, b) => a.arrivedDay - b.arrivedDay);
  const candidate = waiting[0] || null;
  if (candidate) candidate.lastPetition = state.day;

  const attendees = [];
  for (const v of members()) {
    let chance = 0.42;
    if (candidate) chance += perceivedTrust(v, candidate.id) * 0.35;
    if (v.farmstead) chance -= 0.22;
    v.atCommons = rand() < chance;
    if (v.atCommons) attendees.push(v);
  }

  if (candidate) {
    candidate.atCommons = true;
    const sponsors = attendees.filter((v) => perceivedTrust(v, candidate.id) > 0.55);
    const objectors = attendees.filter((v) =>
      perceivedTrust(v, candidate.id) < 0.34 && rand() < 0.3 + p.objectionRate * 0.7);
    const committed = sponsors.length >= p.sponsors
      && attendees.length >= p.witnessQuorum
      && objectors.length <= Math.floor(attendees.length * 0.25);

    addAttestation({
      type: "ceremony-record/1",
      by: "comms.steward:zVILLAGE",
      target: candidate.id,
      detail: { committed, sponsors: sponsors.length, witnesses: attendees.length, objections: objectors.length },
      at: world.commons
    }, [candidate, ...attendees]);

    if (committed) {
      candidate.member = true;
      candidate.joinedDay = state.day;
      candidate.sponsors = sponsors.slice(0, p.sponsors).map((s) => s.id);
      candidate.home = claimHome(world.homes);
      for (const s of sponsors.slice(0, p.sponsors)) {
        addAttestation({
          type: "endorsement/1",
          by: s.id, target: candidate.id,
          detail: { in_capacity: "coming-into-community", weight: "primary" },
          at: world.commons
        }, [candidate, ...attendees]);
      }
      logEvent("ceremony-record/1", `${candidate.label} admitted at the commons`,
        `${sponsors.length} sponsors, ${attendees.length} witnesses, ${objectors.length} objections. Only those present hold the record.`);
    } else {
      candidate.failedPetitions = (candidate.failedPetitions || 0) + 1;
      for (const o of objectors) {
        addAttestation({
          type: "objection/1",
          by: o.id, target: candidate.id,
          detail: { kind: "procedural", grounds: "ceremony held" },
          at: world.commons
        }, [candidate, ...attendees]);
      }
      logEvent("objection/1", `${candidate.label} not admitted`,
        failureReason(p, sponsors.length, attendees.length, objectors.length));
      if (candidate.failedPetitions >= 4) {
        state.villagers = state.villagers.filter((v) => v !== candidate);
        state.byId.delete(candidate.id);
        logEvent("ceremony-record/1", `${candidate.label} moved on`,
          "Four ceremonies held without commitment; they took the south road at dawn.");
      }
    }
  }

  gossipAmong(attendees.concat(candidate ? [candidate] : []), p);
}

// Evening: everyone home; news moves between neighboring hearths. The
// farmstead gossips intensely among itself — tight inside, lagged outside.
function phaseEvening(p) {
  for (const v of state.villagers) { v.atMarket = false; v.atCommons = false; }
  gossipByProximity(state.villagers, p);

  if (rand() < p.arrivalRate * 0.55) {
    arrive("honest");
  }
}

function failureReason(p, sponsors, witnesses, objections) {
  if (sponsors < p.sponsors) return `Only ${sponsors} villagers trusted them enough to sponsor; the rule asks ${p.sponsors}.`;
  if (witnesses < p.witnessQuorum) return `Only ${witnesses} came to witness; quorum is ${p.witnessQuorum}.`;
  return `${objections} objections at the commons stalled it.`;
}

function arrive(archetype) {
  const v = makeVillager(state.nextVillager++, {
    member: false,
    archetype,
    home: { ...world.camp },
    pos: { x: 0.14, y: 0.99 }
  });
  if (archetype === "defector") v.capability = clamp(0.85 + rand() * 0.3, 0.2, 1.15);
  v.arrivedDay = state.day;
  state.villagers.push(v);
  state.byId.set(v.id, v);
  logEvent("ceremony-record/1", `${v.label} arrived by the south road`,
    archetype === "defector"
      ? "A well-presented newcomer. The pledges will tell."
      : "A newcomer makes camp by the commons and waits to petition.");
  return v;
}

function injectDefector() {
  arrive("defector");
  renderStatic();
}

// --- Gossip -----------------------------------------------------------------------
// Word of mouth: two villagers within reach trade the most recent news the
// other lacks, a few items at a time. Recency-first sharing is what makes
// fresh scandal outrun old history.

function exchange(a, b, depth) {
  let moved = 0;
  moved += shareRecent(a, b, depth);
  moved += shareRecent(b, a, depth);
  return moved;
}

function shareRecent(from, to, depth) {
  let shared = 0;
  for (let i = from.feed.length - 1; i >= 0 && shared < depth; i -= 1) {
    if (learn(to, from.feed[i])) shared += 1;
  }
  return shared;
}

// Each villager talks to at most a couple of people per phase. This is the
// load-bearing constraint: all-pairs gossip synchronizes the whole village in
// a day and erases partial knowledge entirely (verified by the harness).
function gossipAmong(group, p) {
  for (const v of group) {
    const partners = nearestOthers(group, v, 4);
    let talks = 0;
    for (const other of partners) {
      if (talks >= 2) break;
      if (rand() < 0.55) {
        talks += 1;
        if (exchange(v, other, p.gossipDepth) > 0) ripple(v, other);
      }
    }
  }
}

function gossipByProximity(group, p) {
  for (const v of group) {
    const neighbors = group
      .filter((o) => o !== v && dist(v.home, o.home) <= p.gossipRadius)
      .sort((a, b) => dist(v.home, a.home) - dist(v.home, b.home))
      .slice(0, 3);
    let talks = 0;
    for (const other of neighbors) {
      if (talks >= 2) break;
      if (rand() < 0.6) {
        talks += 1;
        if (exchange(v, other, p.gossipDepth) > 0) ripple(v, other);
      }
    }
  }
}

function ripple(a, b) {
  if (pulses.length > 140) return;
  pulses.push({
    kind: "gossip",
    x: (a.pos.x + b.pos.x) / 2,
    y: (a.pos.y + b.pos.y) / 2,
    t: 0,
    color: "#4f8f8b"
  });
}

function nearestOthers(group, v, n) {
  return group.filter((o) => o !== v)
    .sort((a, b) => dist(v.pos, a.pos) - dist(v.pos, b.pos))
    .slice(0, n);
}

function members() {
  return state.villagers.filter((v) => v.member);
}

// --- Movement targets ----------------------------------------------------------------

function assignTargets(p) {
  const phase = PHASES[state.phase].key;
  for (const v of state.villagers) {
    if (!v.member) {
      v.target = phase === "market" && v.atMarket
        ? jitterAround(world.market, world.market.r * 0.95)
        : phase === "commons" && v.atCommons
          ? jitterAround(world.commons, world.commons.r * 0.8)
          : jitterAround(world.camp, 0.02);
      continue;
    }
    if (phase === "market" && v.atMarket) {
      v.target = jitterAround(world.market, world.market.r * 0.8);
    } else if (phase === "commons" && v.atCommons) {
      v.target = jitterAround(world.commons, world.commons.r * 0.75);
    } else {
      v.target = jitterAround(v.home, 0.008);
    }
  }
}

function jitterAround(pt, r) {
  const a = rand() * Math.PI * 2;
  const d = Math.sqrt(rand()) * r;
  return { x: pt.x + Math.cos(a) * d, y: pt.y + Math.sin(a) * d };
}

function snapPositions() {
  for (const v of state.villagers) v.pos = { ...v.target };
}

// --- Derived metrics (cached per phase, not per frame) ----------------------------------

function refreshCaches() {
  const ms = members();

  // Coverage of *recent* news (last 3 days). Old attestations eventually
  // reach everyone — that's correct, not interesting. The lag lives at the
  // fresh end of the log.
  let firstRecent = state.attestations.length;
  for (let i = state.attestations.length - 1; i >= 0; i -= 1) {
    if (state.attestations[i].day < state.day - 2) break;
    firstRecent = i;
  }
  const recentCount = state.attestations.length - firstRecent;
  let coverage = 0;
  if (recentCount > 0 && ms.length) {
    for (const v of ms) {
      let known = 0;
      for (let i = firstRecent; i < state.attestations.length; i += 1) {
        if (v.knowledge.has(i)) known += 1;
      }
      coverage += known / recentCount;
    }
    coverage /= ms.length;
  } else {
    coverage = 1;
  }

  // Belief spread: how much the village disagrees about its own members.
  const mean = new Map();
  let spread = 0;
  let counted = 0;
  for (const subject of state.villagers) {
    const views = [];
    for (const viewer of ms) {
      if (viewer === subject) continue;
      views.push(perceivedTrust(viewer, subject.id));
    }
    if (!views.length) continue;
    const m = views.reduce((x, y) => x + y, 0) / views.length;
    mean.set(subject.id, m);
    if (subject.member) {
      const sd = Math.sqrt(views.reduce((s, x) => s + (x - m) ** 2, 0) / views.length);
      spread += sd;
      counted += 1;
    }
  }
  state.cached = { coverage, spread: counted ? spread / counted : 0, mean };
}

function logEvent(type, title, body) {
  state.events.unshift({ day: state.day, phase: PHASES[state.phase].label, type, title, body });
  state.events = state.events.slice(0, 40);
}

// --- Rendering ------------------------------------------------------------------------

const ui = hasDom ? {
  canvas: byId("worldCanvas"),
  clock: byId("clock"),
  runBtn: byId("runBtn"),
  viewpointPill: byId("viewpointPill"),
  inspector: byId("inspector"),
  eventLog: byId("eventLog"),
  membersMetric: byId("membersMetric"),
  attestMetric: byId("attestMetric"),
  coverageMetric: byId("coverageMetric"),
  spreadMetric: byId("spreadMetric")
} : null;

function scaledContext(canvas) {
  const ratio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(canvas.clientWidth * ratio));
  const height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const ctx = canvas.getContext("2d");
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  return ctx;
}

function colorForTrust(t) {
  if (t > 0.72) return "#2f7d66";
  if (t > 0.48) return "#6a8f5a";
  if (t > 0.3) return "#b87815";
  return "#b65345";
}

function drawHouse(ctx, x, y) {
  ctx.fillStyle = "#cbb592";
  ctx.fillRect(x - 6, y - 4, 12, 9);
  ctx.strokeStyle = "rgba(96, 80, 52, 0.55)";
  ctx.lineWidth = 1;
  ctx.strokeRect(x - 6, y - 4, 12, 9);
  ctx.beginPath();
  ctx.moveTo(x - 7.5, y - 4);
  ctx.lineTo(x, y - 11);
  ctx.lineTo(x + 7.5, y - 4);
  ctx.closePath();
  ctx.fillStyle = "#a05f43";
  ctx.fill();
}

function drawWorld(ctx, w, h) {
  // Meadow
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, "#c3d2ab");
  grad.addColorStop(1, "#aec39a");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);

  // Fields
  for (const f of world.fields) {
    ctx.save();
    ctx.translate(f.x * w, f.y * h);
    ctx.rotate(f.angle);
    const fw = f.w * w;
    const fh = f.h * h;
    ctx.fillStyle = "#cdbd86";
    ctx.fillRect(-fw / 2, -fh / 2, fw, fh);
    ctx.strokeStyle = "rgba(122, 104, 56, 0.35)";
    ctx.lineWidth = 1;
    for (let i = 1; i < 6; i += 1) {
      const y = -fh / 2 + (fh * i) / 6;
      ctx.beginPath();
      ctx.moveTo(-fw / 2 + 3, y);
      ctx.lineTo(fw / 2 - 3, y);
      ctx.stroke();
    }
    ctx.strokeStyle = "rgba(90, 78, 44, 0.5)";
    ctx.strokeRect(-fw / 2, -fh / 2, fw, fh);
    ctx.restore();
  }

  // Roads
  for (const road of world.roads) {
    ctx.beginPath();
    ctx.moveTo(road[0].x * w, road[0].y * h);
    for (const pt of road.slice(1)) ctx.lineTo(pt.x * w, pt.y * h);
    ctx.strokeStyle = "#d9c9a4";
    ctx.lineWidth = 9;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.stroke();
    ctx.strokeStyle = "rgba(124, 106, 70, 0.4)";
    ctx.lineWidth = 1;
    ctx.setLineDash([5, 7]);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Commons: a stone circle, the ceremony ground.
  const c = world.commons;
  ctx.beginPath();
  ctx.arc(c.x * w, c.y * h, c.r * w, 0, Math.PI * 2);
  ctx.fillStyle = "#d6d2bb";
  ctx.fill();
  ctx.strokeStyle = "rgba(96, 92, 70, 0.6)";
  ctx.lineWidth = 1.5;
  ctx.stroke();
  for (let i = 0; i < 9; i += 1) {
    const a = (Math.PI * 2 * i) / 9;
    ctx.beginPath();
    ctx.arc(c.x * w + Math.cos(a) * c.r * w * 0.82, c.y * h + Math.sin(a) * c.r * w * 0.82, 2.6, 0, Math.PI * 2);
    ctx.fillStyle = "#8b8674";
    ctx.fill();
  }

  // Market: stalls with goods-colored awnings.
  const m = world.market;
  ctx.beginPath();
  ctx.arc(m.x * w, m.y * h, m.r * w, 0, Math.PI * 2);
  ctx.fillStyle = "#dccfae";
  ctx.fill();
  ctx.strokeStyle = "rgba(124, 106, 70, 0.45)";
  ctx.stroke();
  for (const stall of world.stalls) {
    const sx = stall.x * w;
    const sy = stall.y * h;
    ctx.fillStyle = "#8a6a45";
    ctx.fillRect(sx - 6, sy - 3, 12, 7);
    ctx.fillStyle = stall.good.color;
    ctx.beginPath();
    ctx.moveTo(sx - 8, sy - 3);
    ctx.lineTo(sx + 8, sy - 3);
    ctx.lineTo(sx + 6, sy - 9);
    ctx.lineTo(sx - 6, sy - 9);
    ctx.closePath();
    ctx.fill();
  }

  // Newcomers' camp
  const camp = world.camp;
  ctx.beginPath();
  ctx.moveTo(camp.x * w - 8, camp.y * h + 6);
  ctx.lineTo(camp.x * w, camp.y * h - 8);
  ctx.lineTo(camp.x * w + 8, camp.y * h + 6);
  ctx.closePath();
  ctx.fillStyle = "#c8b48e";
  ctx.fill();
  ctx.strokeStyle = "rgba(96, 80, 52, 0.6)";
  ctx.stroke();

  // Homes (village ring + farmstead), claimed plots only.
  for (const pool of [world.homes, world.farmHomes]) {
    for (const homePlot of pool) {
      if (!homePlot.claimed) continue;
      drawHouse(ctx, homePlot.x * w, homePlot.y * h);
    }
  }

  // Trees
  for (const t of world.trees) {
    ctx.beginPath();
    ctx.arc(t.x * w, t.y * h, t.r * w, 0, Math.PI * 2);
    ctx.fillStyle = "#7fa06b";
    ctx.fill();
    ctx.beginPath();
    ctx.arc(t.x * w - t.r * w * 0.3, t.y * h - t.r * w * 0.35, t.r * w * 0.62, 0, Math.PI * 2);
    ctx.fillStyle = "#92b27c";
    ctx.fill();
  }

  // Labels
  ctx.fillStyle = "rgba(45, 50, 40, 0.72)";
  ctx.font = "italic 12px Georgia, 'Iowan Old Style', serif";
  ctx.fillText("the commons", c.x * w - 32, (c.y - c.r) * h - 8);
  ctx.fillText("market square", m.x * w - 36, (m.y + m.r) * h + 18);
  ctx.fillText("the farmstead", world.farmstead.x * w - 36, (world.farmstead.y - 0.085) * h);
  ctx.fillText("newcomers' camp", camp.x * w - 44, camp.y * h + 24);
}

function viewpoint() {
  return selectedId ? state.byId.get(selectedId) : null;
}

function drawVillagers(ctx, w, h) {
  const vp = viewpoint();
  for (const v of state.villagers) {
    const x = v.pos.x * w;
    const y = v.pos.y * h;
    const r = 5 + v.capability * 4;

    let fill;
    let faded = false;
    if (vp) {
      if (v.id === vp.id) {
        fill = colorForTrust(state.cached.mean.get(v.id) ?? state.prior);
      } else if (isStrangerTo(vp, v.id)) {
        fill = "#9aa39b";
        faded = true;
      } else {
        fill = colorForTrust(perceivedTrust(vp, v.id));
      }
    } else {
      fill = v.member ? colorForTrust(state.cached.mean.get(v.id) ?? state.prior) : "#b87815";
    }

    ctx.globalAlpha = faded ? 0.45 : 1;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.lineWidth = v.member ? 1.4 : 1;
    ctx.strokeStyle = v.member ? "rgba(30, 37, 39, 0.55)" : "rgba(30, 37, 39, 0.3)";
    if (!v.member) ctx.setLineDash([2, 2]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;

    // Ground truth is a debug privilege: defectors are only marked when no
    // villager's viewpoint is selected.
    if (!vp && v.archetype === "defector") {
      ctx.save();
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      ctx.arc(x, y, r + 3, 0, Math.PI * 2);
      ctx.strokeStyle = "rgba(94, 53, 110, 0.9)";
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.restore();
    }

    if (vp && v.id === vp.id) {
      ctx.beginPath();
      ctx.arc(x, y, r + 4, 0, Math.PI * 2);
      ctx.strokeStyle = "#1e2527";
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.fillStyle = "#1e2527";
      ctx.font = "bold 12px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 6, y + 4);
    } else if (!vp && (v.capability > 0.95 || !v.member)) {
      ctx.fillStyle = "rgba(30, 37, 39, 0.75)";
      ctx.font = "11px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 4, y + 4);
    }
  }
}

function drawPulses(ctx, w, h, dt) {
  for (const pulse of pulses) {
    pulse.t += dt;
    const life = pulse.kind === "gossip" ? 1.4 : 1.8;
    const k = pulse.t / life;
    if (k >= 1) continue;
    const alpha = (1 - k) * (pulse.kind === "gossip" ? 0.5 : 0.85);
    ctx.beginPath();
    ctx.arc(pulse.x * w, pulse.y * h, 3 + k * (pulse.kind === "gossip" ? 22 : 14), 0, Math.PI * 2);
    ctx.strokeStyle = hexToRgba(pulse.color, alpha);
    ctx.lineWidth = pulse.kind === "gossip" ? 1.2 : 2;
    ctx.stroke();
  }
  pulses = pulses.filter((pulse) => pulse.t < 2);
}

function hexToRgba(hex, a) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a.toFixed(3)})`;
}

function moveVillagers(dt) {
  const speed = 0.14;
  for (const v of state.villagers) {
    const dx = v.target.x - v.pos.x;
    const dy = v.target.y - v.pos.y;
    const d = Math.hypot(dx, dy);
    const stepLen = Math.min(d, speed * dt);
    if (d > 0.0005) {
      v.pos.x += (dx / d) * stepLen;
      v.pos.y += (dy / d) * stepLen;
    }
    // A faint idle sway so the village never looks frozen.
    const t = performance.now() / 1000 + v.wander;
    v.pos.x += Math.sin(t * 0.9) * 0.00018;
    v.pos.y += Math.cos(t * 0.7) * 0.00015;
  }
}

function phaseDuration(speed) {
  return 3.4 - speed * 0.29; // seconds per phase, ~3.1s at pace 1, ~0.5s at 10
}

function frame(now) {
  const dt = Math.min(0.05, (now - lastFrame) / 1000 || 0.016);
  lastFrame = now;

  if (running) {
    phaseClock += dt;
    const p = params();
    if (phaseClock >= phaseDuration(p.speed)) {
      phaseClock = 0;
      nextPhase(p);
      renderStatic();
    }
  }

  moveVillagers(dt);
  const ctx = scaledContext(ui.canvas);
  const w = ui.canvas.clientWidth;
  const h = ui.canvas.clientHeight;
  drawWorld(ctx, w, h);
  drawPulses(ctx, w, h, dt);
  drawVillagers(ctx, w, h);

  requestAnimationFrame(frame);
}

// DOM-side panels: only re-rendered on phase changes or interaction.
function renderStatic() {
  ui.clock.textContent = `Day ${state.day} · ${PHASES[state.phase].label}`;
  ui.membersMetric.textContent = String(members().length);
  ui.attestMetric.textContent = String(state.attestations.length);
  ui.coverageMetric.textContent = `${Math.round(state.cached.coverage * 100)}%`;
  ui.spreadMetric.textContent = state.cached.spread.toFixed(2);
  ui.runBtn.textContent = running ? "Pause" : "Run";

  const vp = viewpoint();
  ui.viewpointPill.textContent = vp ? `Seen by ${vp.label}` : "Omniscient view";
  ui.viewpointPill.classList.toggle("active", Boolean(vp));

  renderLog();
  renderInspector();
}

function renderLog() {
  ui.eventLog.innerHTML = state.events.map((e) => `
    <li class="evt">
      <span class="evt-tag" style="background:${hexToRgba(attColors[e.type] || "#637074", 0.16)};color:${attColors[e.type] || "#637074"}">d${e.day}</span>
      <div><strong>${e.title}</strong><p>${e.body}</p></div>
    </li>
  `).join("");
}

// The field-notebook card. In viewpoint mode it reports what this villager
// knows, and where their beliefs diverge most from the village mean — the
// readable trace of partial knowledge.
function renderInspector() {
  const v = viewpoint();
  if (!v) {
    ui.inspector.hidden = true;
    ui.inspector.innerHTML = "";
    return;
  }
  const total = Math.max(1, state.attestations.length);
  const coverage = Math.round((v.knowledge.size / total) * 100);

  const divergences = state.villagers
    .filter((o) => o !== v && o.member && !isStrangerTo(v, o.id))
    .map((o) => {
      const mine = perceivedTrust(v, o.id);
      const villageMean = state.cached.mean.get(o.id) ?? mine;
      return { o, mine, gap: mine - villageMean };
    })
    .sort((a, b) => Math.abs(b.gap) - Math.abs(a.gap))
    .slice(0, 3);

  const strangers = state.villagers.filter((o) => isStrangerTo(v, o.id)).length;

  ui.inspector.hidden = false;
  ui.inspector.innerHTML = `
    <button type="button" class="inspector-close" aria-label="Return to omniscient view">&times;</button>
    <div class="inspector-head">
      <strong>${v.label}</strong>
      <span>${v.member ? (v.farmstead ? "farmstead member" : "village member") : "newcomer"} · ${v.specialty.id}</span>
    </div>
    <dl class="inspector-stats">
      <div><dt>knows</dt><dd>${v.knowledge.size} of ${total} attestations (${coverage}%)</dd></div>
      <div><dt>strangers to them</dt><dd>${strangers}</dd></div>
      <div><dt>capability</dt><dd>${v.capability.toFixed(2)}</dd></div>
      <div><dt>${v.member ? "joined" : "arrived"}</dt><dd>day ${v.member ? v.joinedDay : v.arrivedDay}</dd></div>
    </dl>
    ${divergences.length ? `
      <p class="inspector-sub">Where ${v.label} disagrees with the village:</p>
      <ul class="inspector-beliefs">
        ${divergences.map((d) => `
          <li><i style="background:${colorForTrust(d.mine)}"></i>${d.o.label}:
            sees ${Math.round(d.mine * 100)}%, village mean ${Math.round((d.mine - d.gap) * 100)}%
            <em>${d.gap > 0.04 ? "(hasn't heard the bad news)" : d.gap < -0.04 ? "(knows something the village doesn't)" : ""}</em>
          </li>`).join("")}
      </ul>` : ""}
    <p class="inspector-note">The whole map is now colored by ${v.label}'s beliefs. Gray villagers are strangers — ${v.label} holds no attestation about them.</p>
  `;
  ui.inspector.querySelector(".inspector-close").addEventListener("click", () => {
    selectedId = null;
    renderStatic();
  });
}

// --- Interaction ---------------------------------------------------------------------

function canvasClick(ev) {
  const rect = ui.canvas.getBoundingClientRect();
  const x = (ev.clientX - rect.left) / rect.width;
  const y = (ev.clientY - rect.top) / rect.height;
  let best = null;
  let bestD = 0.03;
  for (const v of state.villagers) {
    const d = dist({ x, y }, v.pos);
    if (d < bestD) {
      best = v;
      bestD = d;
    }
  }
  selectedId = best ? best.id : null;
  renderStatic();
}

function syncOutputs() {
  const map = {
    population: (v) => v,
    farmShare: (v) => `${v}%`,
    trust: (v) => `${v}%`,
    gossipRadius: (v) => (v <= 8 ? "near" : v <= 16 ? "neighborly" : "far"),
    gossipDepth: (v) => `${v} items`,
    marketAttend: (v) => `${v}%`,
    sponsors: (v) => v,
    witnessQuorum: (v) => v,
    objectionRate: (v) => `${v}%`,
    arrivalRate: (v) => `${v}%`
  };
  for (const key of Object.keys(map)) {
    const out = byId(`${key}Out`);
    if (out) out.textContent = map[key](Number(controls[key].value));
  }
}

function init() {
  for (const key of Object.keys(controls)) {
    controls[key].addEventListener("input", syncOutputs);
  }
  byId("runBtn").addEventListener("click", () => {
    running = !running;
    renderStatic();
  });
  byId("stepBtn").addEventListener("click", stepDay);
  byId("resetBtn").addEventListener("click", () => {
    running = false;
    reset();
  });
  byId("injectBtn").addEventListener("click", injectDefector);
  ui.canvas.addEventListener("click", canvasClick);

  syncOutputs();
  seedState(params());
  snapPositions();
  renderStatic();
  requestAnimationFrame(frame);
}

if (hasDom && byId("worldCanvas")) {
  init();
}
