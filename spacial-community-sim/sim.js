// Comms Village — sim.js
//
// The simulation proper: villagers, attestations, beliefs, the four-phase
// day, gossip, and metrics. The epistemic core is UNCHANGED from prototype 2
// and follows the protocol's stance: every trust input is an attestation,
// born at a place, known only to whoever was present; trust is a pure
// function of the attestations a viewer actually holds, plus the community
// prior. There is no global trust scalar anywhere in this project.
//
// What changed in this revision is PRESENCE. Whether a villager shows up at
// the market or the commons is no longer a flag (`farmstead`) but a function
// of travelCost(home, venue) over the real terrain, so the farmstead's
// information lag — and anyone else's — is emergent from the map. Phase
// logic now also consumes positions honestly: attendees are assigned spots
// in the venue first, and witnesses and gossip partners are chosen from
// those spots, not from wherever people happened to be standing last phase.
//
// Headless contract (unchanged): seedState(p), nextPhase(p), advanceDay(p),
// and normalizeParams(raw) run without a DOM. A harness concatenates
// world.js + sim.js + render.js + ui.js with test code and drives scenarios.

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

// Same attack vocabulary as the original simulator. Values are probabilities
// except activationDelay (days). The spatial harness runs every preset against
// both flat tallying and the Vouch reference profile.
const ADVERSARY_PRESETS = {
  classic:     { activationDelay: 0, defectRate: .55, selectivity: 0, coverRate: 0, socialInvestment: 0, recoveryRate: 0, networkSubversion: 0 },
  sleeper:     { activationDelay: 30, defectRate: .70, selectivity: 0, coverRate: .20, socialInvestment: .20, recoveryRate: 0, networkSubversion: 0 },
  selective:   { activationDelay: 5, defectRate: .60, selectivity: .80, coverRate: .40, socialInvestment: 0, recoveryRate: 0, networkSubversion: 0 },
  parasite:    { activationDelay: 0, defectRate: .20, selectivity: .20, coverRate: .50, socialInvestment: 0, recoveryRate: .40, networkSubversion: 0 },
  charmer:     { activationDelay: 8, defectRate: .65, selectivity: .40, coverRate: .40, socialInvestment: .80, recoveryRate: .30, networkSubversion: 0 },
  ghost:       { activationDelay: 3, defectRate: .55, selectivity: .50, coverRate: 1, socialInvestment: 0, recoveryRate: 1, networkSubversion: 0 },
  freeRider:   { activationDelay: 0, defectRate: .15, selectivity: 0, coverRate: .40, socialInvestment: .30, recoveryRate: .50, networkSubversion: 0 },
  cultivator:  { activationDelay: 5, defectRate: .40, selectivity: .30, coverRate: .70, socialInvestment: 1, recoveryRate: .80, networkSubversion: 0 },
  factionist:  { activationDelay: 10, defectRate: .40, selectivity: .70, coverRate: .60, socialInvestment: .60, recoveryRate: .40, networkSubversion: 1 },
  infiltrator: { activationDelay: 40, defectRate: .50, selectivity: .80, coverRate: .80, socialInvestment: .80, recoveryRate: .60, networkSubversion: .70 },
  ideologue:   { activationDelay: 5, defectRate: .50, selectivity: .75, coverRate: .30, socialInvestment: .50, recoveryRate: .20, networkSubversion: .90 },
  brinksman:   { activationDelay: 5, defectRate: .70, selectivity: 1, coverRate: .75, socialInvestment: .20, recoveryRate: .50, networkSubversion: 0 },
  flash:       { activationDelay: 0, defectRate: .90, selectivity: 0, coverRate: 0, socialInvestment: 0, recoveryRate: 0, networkSubversion: 0 },
  patriarch:   { activationDelay: 50, defectRate: .60, selectivity: .50, coverRate: .80, socialInvestment: 1, recoveryRate: .50, networkSubversion: .60 },
  wrecker:     { activationDelay: 0, defectRate: .65, selectivity: 0, coverRate: 0, socialInvestment: .20, recoveryRate: 0, networkSubversion: .80 },
  sovereign:   { activationDelay: 15, defectRate: .75, selectivity: .70, coverRate: .70, socialInvestment: .70, recoveryRate: .60, networkSubversion: .70 }
};

// --- Parameters ---------------------------------------------------------------

// Normalizes a raw control reading (or a hand-built object) into simulation
// units, so parameter sets can be constructed off-DOM. `travelWill` replaces
// the old farmstead-only `marketAttend`: it is everyone's tolerance for a
// costly trip. The old key is still accepted so existing harness scenarios
// keep running.
function normalizeParams(raw) {
  return {
    population: Math.round(raw.population ?? 18),
    farmShare: (raw.farmShare ?? 25) / 100,
    trust: (raw.trust ?? 55) / 100,
    gossipRadius: (raw.gossipRadius ?? 12) / 100,   // world units (map is 1x1)
    gossipDepth: Math.round(raw.gossipDepth ?? 4),
    travelWill: ((raw.travelWill ?? raw.marketAttend) ?? 25) / 100,
    sponsors: Math.round(raw.sponsors ?? 2),
    witnessQuorum: Math.round(raw.witnessQuorum ?? 5),
    objectionRate: (raw.objectionRate ?? 40) / 100,
    arrivalRate: (raw.arrivalRate ?? 35) / 100,
    vouchMode: raw.vouchMode === true || raw.vouchMode === 1,
    seedOffset: Math.round(raw.seedOffset ?? 0),
    speed: raw.speed ?? 5
  };
}

// --- State ---------------------------------------------------------------------

let state;
let fallbackSeed = 4231;

function rand() {
  const seed = state ? state.seed++ : fallbackSeed++;
  const x = Math.sin(seed) * 10000;
  return x - Math.floor(x);
}

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

// --- Villagers -----------------------------------------------------------------

function makeVillager(index, opts = {}) {
  const farm = opts.farmstead === true;
  const home = opts.home || claimHome(farm ? world.farmHomes : world.homes);
  return {
    id: `comms.steward:z${String(index + 1).padStart(3, "0")}`,
    label: names[index % names.length],
    member: opts.member !== false,
    archetype: opts.archetype || "honest",
    farmstead: farm,                  // descriptive only; presence is cost-driven
    specialty: goodsTable[index % goodsTable.length],
    capability: clamp(0.45 + rand() * 0.7, 0.2, 1.15),
    stock: 1 + rand(),
    cart: false,                      // params hook for the cost model; nobody
                                      // owns one yet — the courier tier will
    home,                             // a plot reference, not a copy
    pos: opts.pos ? { ...opts.pos } : { x: home.x, y: home.y },
    target: { x: home.x, y: home.y },
    journey: null,
    arrivedDay: state ? state.day : 0,
    joinedDay: opts.member !== false ? 0 : null,
    sponsors: [],
    atMarket: false,
    atCommons: false,
    commute: 0,                       // travel cost of the current trip
    late: 0,                          // 0..1 fraction of the phase spent traveling
    spot: null,                       // assigned position inside the venue
    // The villager's whole epistemic world: which attestations they hold
    // (feed preserves learn order so gossip can share recent news first) and
    // the per-subject tallies those attestations produce.
    knowledge: new Set(),
    feed: [],
    beliefs: new Map(),
    ap: opts.ap || null,
    adversaryType: opts.adversaryType || null,
    lyingLowUntil: 0
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
  if (fields.at && pulses.length < 140) {
    pulses.push({ kind: "attest", x: fields.at.x, y: fields.at.y, t: 0, color: attColors[fields.type] || "#637074" });
  }
  return idx;
}

function recordInteraction(kind, participants, detail = {}) {
  const ids = [...new Set(participants.map((v) => typeof v === "string" ? v : v.id))]
    .filter((id) => state.byId.has(id));
  if (ids.length < 2) return;
  state.interactions.push({ kind, participants: ids, ...detail });
}

function perceivedTrust(viewer, subjectId) {
  const subject = state.byId.get(subjectId);
  if (!subject) return 0;
  const b = viewer.beliefs.get(subjectId) || { pos: 0, neg: 0 };
  const prior = state.prior * (subject.member ? 1 : 0.72);
  const w = 6;
  return clamp((b.pos + prior * w) / (b.pos + b.neg + w), 0.02, 0.99);
}

// Informative Vouch profile: direct interaction issuers and endorsers remain
// separate, repetition by one issuer is capped, and absence is awaiting
// context rather than rejection.
function perceivedVouch(viewer, subjectId) {
  const positive = new Set();
  const negative = new Set();
  const endorsers = new Set();
  let challenged = false;
  for (const idx of viewer.knowledge) {
    const att = state.attestations[idx];
    if (att.target !== subjectId) continue;
    if (att.type === "deal-record/1") {
      (att.detail.outcome === "completed" ? positive : negative).add(att.by);
    } else if (att.type === "endorsement/1") {
      endorsers.add(att.by);
    } else if (att.type === "objection/1") {
      challenged = true;
    }
  }
  const support = positive.size >= 2;
  const reject = negative.size >= 2;
  let outcome = "awaiting-context";
  if ((support && reject) || (challenged && support)) outcome = "contested";
  else if (reject) outcome = "rejected";
  else if (support) outcome = "trusted";
  return { outcome, positive: positive.size, negative: negative.size, endorsers: endorsers.size };
}

function isStrangerTo(viewer, subjectId) {
  return viewer.id !== subjectId && !viewer.beliefs.has(subjectId);
}

// --- Presence model ---------------------------------------------------------------
// The bridge between the cost field and the day. A trip's cost sets both the
// chance of making it at all and, for those who do, how much of the phase is
// burned in transit (lateness shrinks the interaction budget). One falloff
// for everyone: the farmstead lags because the farmstead is far.

function commuteTau(p) {
  // Calibrated against prototype-2 behavior: at the default slider (25%),
  // tau = 0.34 gives near-village households ~0.85 market attendance and the
  // farmstead ~0.27 — the old hardcoded numbers, now produced by geography.
  return 0.16 + p.travelWill * 0.72;
}

function attendFalloff(tripCost, tau) {
  return Math.exp(-((tripCost / tau) ** 2));
}

function planTrip(v, venue, p, tauScale = 1) {
  v.commute = travelCost(v.home, venue, { cart: v.cart });
  v.late = clamp(v.commute / (commuteTau(p) * tauScale * 2.2), 0, 1);
  return attendFalloff(v.commute, commuteTau(p) * tauScale);
}

function placeAttendees(group, venue, spread) {
  for (const v of group) v.spot = jitterAround(venue, venue.r * spread);
}

// --- Seeding -------------------------------------------------------------------

function seedState(p) {
  world = buildWorld(p);
  state = {
    day: 1,
    phase: 0,
    seed: 2741 + p.population * 13 + Math.round(p.trust * 100) + p.seedOffset * 1009,
    prior: p.trust,
    villagers: [],
    byId: new Map(),
    attestations: [],
    events: [],
    interactions: [],
    interactionOverlay: { direct: [], evidence: [] },
    nextId: 1,
    nextVillager: 0,
    cached: { coverage: 0, spread: 0, mean: new Map() }
  };
  pulses = [];

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

function runPhaseLogic(p, phase) {
  state.interactions = [];
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
// at the moment it is made. Attendance falls off with trip cost, so far
// households (the farmstead, but equally an outlying new build) make the
// trip less often and arrive with less of the phase left: that is exactly
// how their knowledge falls behind.
function phaseMarket(p) {
  for (const v of state.villagers) {
    v.atCommons = false;
    v.spot = null;
    if (v.member) {
      v.atMarket = rand() < 0.95 * planTrip(v, world.market, p);
    } else {
      // Newcomers camp close by and are motivated: they hang around the
      // square making themselves useful most days.
      v.commute = travelCost(world.camp, world.market);
      v.late = 0;
      v.atMarket = rand() < 0.7;
    }
  }
  const attendees = state.villagers.filter((v) => v.atMarket);
  placeAttendees(attendees, world.market, 0.85);
  const sellers = attendees.filter((v) => v.member);

  for (const buyer of sellers) {
    if (rand() < buyer.late * 0.45) continue;   // arrived with the phase half spent
    const options = sellers.filter((s) => {
      if (s === buyer || s.specialty === buyer.specialty || s.stock <= 0.5) return false;
      if (!p.vouchMode) return true;
      const judgment = perceivedVouch(buyer, s.id).outcome;
      return judgment !== "rejected" && judgment !== "contested";
    });
    const seller = weightedPick(options, (s) => s.stock);
    if (!seller) continue;
    const witnesses = nearestBySpot(attendees, buyer, 2);
    let failed;
    if (seller.archetype === "adversary" && seller.member) {
      failed = adversaryBetray(seller, buyer, witnesses);
    } else {
      const cheat = seller.archetype === "defector" && seller.member;
      failed = cheat ? rand() < 0.55 : rand() < 0.07;
    }
    seller.stock = Math.max(0, seller.stock - 0.8);
    const dealIdx = addAttestation({
      type: "deal-record/1",
      by: buyer.id,
      target: seller.id,
      detail: { outcome: failed ? "failed" : "completed", good: seller.specialty.id },
      at: world.market
    }, [buyer, seller, ...witnesses]);
    recordInteraction("deal", [buyer, seller, ...witnesses], {
      primary: [buyer.id, seller.id],
      decisionMaker: buyer.id,
      target: seller.id,
      attestation: dealIdx,
      outcome: failed ? "failed" : "completed"
    });
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
      if (seller.archetype === "adversary") adversaryMaybeLieLow(seller);
    } else if (seller.archetype === "adversary" && adversaryIsActive(seller)
               && rand() < seller.ap.socialInvestment * 0.5) {
      addAttestation({
        type: "endorsement/1",
        by: buyer.id,
        target: seller.id,
        detail: { in_capacity: "market-deal", weight: "secondary" },
        at: world.market
      }, [buyer, seller, ...witnesses]);
    }
  }

  // Newcomers earn their first reputations by helping out around the stalls.
  for (const cand of attendees.filter((v) => !v.member)) {
    if (rand() < 0.75) {
      const helped = pick(sellers);
      if (!helped) continue;
      const witnesses = nearestBySpot(attendees, helped, 2);
      const dealIdx = addAttestation({
        type: "deal-record/1",
        by: helped.id,
        target: cand.id,
        detail: { outcome: rand() < 0.92 ? "completed" : "failed", good: "labor" },
        at: world.market
      }, [helped, cand, ...witnesses]);
      recordInteraction("help", [helped, cand, ...witnesses], {
        primary: [helped.id, cand.id],
        decisionMaker: helped.id,
        target: cand.id,
        attestation: dealIdx,
        outcome: state.attestations[dealIdx].detail.outcome
      });
    }
  }

  gossipAmong(attendees, p);
}

// Afternoon: if a newcomer has waited long enough, the village gathers at the
// commons. Sponsorship and objection are judged from EACH attendee's own
// beliefs — the record of the ceremony is then known only to those who came.
// The commons sits west of the square, so the map has two information
// basins: the east side is market-rich, the west side is ceremony-rich, and
// gossip bridges them. Trip cost gates attendance here too.
function phaseCommons(p) {
  for (const v of state.villagers) { v.atMarket = false; v.spot = null; }

  const waiting = state.villagers
    .filter((v) => !v.member
      && state.day - v.arrivedDay >= 2
      && state.day - (v.lastPetition || 0) >= 2)
    .sort((a, b) => a.arrivedDay - b.arrivedDay);
  const candidate = waiting[0] || null;
  if (candidate) candidate.lastPetition = state.day;

  const attendees = [];
  for (const v of members()) {
    const falloff = planTrip(v, world.commons, p, 1.15);  // ceremonies pull harder
    let eagerness = 0.48;
    if (candidate) eagerness += perceivedTrust(v, candidate.id) * 0.35;
    v.atCommons = rand() < eagerness * falloff;
    if (v.atCommons) attendees.push(v);
  }
  placeAttendees(attendees, world.commons, 0.75);

  if (candidate) {
    candidate.atCommons = true;
    candidate.spot = jitterAround(world.commons, world.commons.r * 0.4);
    const sponsors = attendees.filter((v) => p.vouchMode
      ? perceivedVouch(v, candidate.id).outcome === "trusted"
      : perceivedTrust(v, candidate.id) > 0.55);
    const objectors = attendees.filter((v) =>
      (p.vouchMode
        ? ["rejected", "contested"].includes(perceivedVouch(v, candidate.id).outcome)
        : perceivedTrust(v, candidate.id) < 0.34)
      && rand() < 0.3 + p.objectionRate * 0.7);
    // The flat model lets a cultivated faction nudge the apparent sponsor
    // tally. Vouch refuses the bonus because no distinct signed issuer exists.
    const bonusSponsors = !p.vouchMode && candidate.archetype === "adversary" && candidate.ap
      ? Math.floor(candidate.ap.networkSubversion * 2 * rand())
      : 0;
    const committed = sponsors.length + bonusSponsors >= p.sponsors
      && attendees.length >= p.witnessQuorum
      && objectors.length <= Math.floor(attendees.length * 0.25);

    const ceremonyIdx = addAttestation({
      type: "ceremony-record/1",
      by: "comms.steward:zVILLAGE",
      target: candidate.id,
      detail: {
        committed, sponsors: sponsors.length, apparentSponsors: sponsors.length + bonusSponsors,
        witnesses: attendees.length, objections: objectors.length
      },
      at: world.commons
    }, [candidate, ...attendees]);
    recordInteraction("ceremony", [
      candidate,
      ...sponsors.slice(0, Math.max(p.sponsors, 1)),
      ...objectors.slice(0, 3)
    ], {
      primary: sponsors.length ? [sponsors[0].id, candidate.id] : null,
      decisionMaker: sponsors.length ? sponsors[0].id : null,
      target: candidate.id,
      attestation: ceremonyIdx,
      outcome: committed ? "admitted" : "not-admitted"
    });

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
        releaseHome(candidate);
        state.villagers = state.villagers.filter((v) => v !== candidate);
        state.byId.delete(candidate.id);
        logEvent("ceremony-record/1", `${candidate.label} moved on`,
          "Four ceremonies held without commitment; they took the south road at dawn.");
      }
    }
  }

  gossipAmong(attendees.concat(candidate ? [candidate] : []), p);
}

// Evening: everyone home; news moves between neighboring hearths. Reach is
// now a messaging cost, not a circle: news runs farther along a road and is
// damped by a stand of woods between two houses. The farmstead still gossips
// intensely among itself — tight inside, lagged outside — but that, too, is
// now just geometry.
function phaseEvening(p) {
  for (const v of state.villagers) { v.atMarket = false; v.atCommons = false; v.spot = null; }
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

function arrive(archetype, ap = null, adversaryType = null) {
  const v = makeVillager(state.nextVillager++, {
    member: false,
    archetype,
    home: { ...world.camp },
    pos: { x: 0.14, y: 0.99 },
    ap,
    adversaryType
  });
  if (archetype === "defector" || archetype === "adversary") {
    v.capability = clamp(0.85 + rand() * 0.3, 0.2, 1.15);
  }
  v.arrivedDay = state.day;
  state.villagers.push(v);
  state.byId.set(v.id, v);
  logEvent("ceremony-record/1", `${v.label} arrived by the south road`,
    archetype === "defector" || archetype === "adversary"
      ? "A well-presented newcomer. The pledges will tell."
      : "A newcomer makes camp by the commons and waits to petition.");
  return v;
}

function injectDefector() {
  arrive("defector");
}

function injectAdversary(name = "classic") {
  const preset = ADVERSARY_PRESETS[name];
  if (!preset) throw new Error(`unknown adversary preset: ${name}`);
  return arrive("adversary", { ...preset }, name);
}

function adversaryIsActive(v) {
  if (!v.ap || state.day < v.lyingLowUntil) return false;
  return v.joinedDay !== null
    && state.day - v.joinedDay >= v.ap.activationDelay;
}

function adversaryBetray(seller, buyer, witnesses) {
  if (!adversaryIsActive(seller)) return false;
  if (seller.ap.selectivity > 0) {
    const trust = state.cached.mean.get(buyer.id) ?? 0.5;
    if (trust < seller.ap.selectivity * 0.65) return false;
  }
  const witnessFactor = 1 - seller.ap.coverRate * Math.min(witnesses.length, 4) / 4;
  return rand() < seller.ap.defectRate * witnessFactor;
}

function adversaryMaybeLieLow(v) {
  if (!v.ap || v.ap.recoveryRate <= 0) return;
  const recent = state.attestations.filter((a) =>
    a.type === "objection/1" && a.target === v.id && a.day >= state.day - 4).length;
  if (recent && rand() < v.ap.recoveryRate) {
    v.lyingLowUntil = state.day + Math.max(1, Math.round(v.ap.recoveryRate * 12));
  }
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

// Each villager talks to at most a couple of people per phase — one, if the
// walk in ate half their day. This is the load-bearing constraint: all-pairs
// gossip synchronizes the whole village in a day and erases partial
// knowledge entirely (verified by the harness).
function gossipAmong(group, p) {
  for (const v of group) {
    const partners = nearestBySpot(group, v, 4);
    const budget = v.late > 0.55 ? 1 : 2;
    let talks = 0;
    for (const other of partners) {
      if (talks >= budget) break;
      if (rand() < 0.55) {
        talks += 1;
        const moved = exchange(v, other, p.gossipDepth);
        if (moved > 0) {
          ripple(v, other);
          recordInteraction("gossip", [v, other], {
            primary: [v.id, other.id],
            outcome: `${moved}-records`
          });
        }
      }
    }
  }
}

// Hearth-to-hearth. The cheap-distance prefilter (no cost multiplier is
// below 0.5) keeps this O(N^2) pass light before paying for line integrals.
function gossipByProximity(group, p) {
  for (const v of group) {
    const within = [];
    for (const o of group) {
      if (o === v) continue;
      if (dist(v.home, o.home) * 0.5 > p.gossipRadius) continue;
      const c = messagingCost(v.home, o.home);
      if (c <= p.gossipRadius) within.push({ o, c });
    }
    within.sort((a, b) => a.c - b.c);
    let talks = 0;
    for (const { o } of within.slice(0, 3)) {
      if (talks >= 2) break;
      if (rand() < 0.6) {
        talks += 1;
        const moved = exchange(v, o, p.gossipDepth);
        if (moved > 0) {
          ripple(v, o);
          recordInteraction("gossip", [v, o], {
            primary: [v.id, o.id],
            outcome: `${moved}-records`
          });
        }
      }
    }
  }
}

function nearestBySpot(group, v, n) {
  const at = (x) => x.spot || x.pos;
  const me = at(v);
  return group.filter((o) => o !== v)
    .sort((a, b) => dist(me, at(a)) - dist(me, at(b)))
    .slice(0, n);
}

function members() {
  return state.villagers.filter((v) => v.member);
}

// --- Movement targets ----------------------------------------------------------------
// Targets are where consequences already happened: attendees walk to the
// spot their interactions were computed from, so the animation is a faithful
// replay rather than theater.

function assignTargets(p) {
  const phase = PHASES[state.phase].key;
  for (const v of state.villagers) {
    if (phase === "market" && v.atMarket && v.spot) {
      v.target = { ...v.spot };
    } else if (phase === "commons" && v.atCommons && v.spot) {
      v.target = { ...v.spot };
    } else if (!v.member) {
      // Preserve the historical PRNG schedule without applying visual jitter.
      rand(); rand();
      v.target = { x: world.camp.x, y: world.camp.y };
    } else {
      rand(); rand();
      v.target = { x: v.home.x, y: v.home.y };
    }
  }
}

function jitterAround(pt, r) {
  const a = rand() * Math.PI * 2;
  const d = Math.sqrt(rand()) * r;
  return { x: pt.x + Math.cos(a) * d, y: pt.y + Math.sin(a) * d };
}

function snapPositions() {
  for (const v of state.villagers) {
    v.pos = { ...v.target };
    v.journey = null;
  }
}

function evidenceClass(att) {
  if (att.type === "deal-record/1") {
    return att.detail.outcome === "completed" ? "positive" : "negative";
  }
  if (att.type === "objection/1") return "negative";
  if (att.type === "endorsement/1") return "endorsement";
  if (att.type === "ceremony-record/1") return "ceremony";
  return "context";
}

function buildInteractionOverlay(viewer, p) {
  const direct = [];
  const evidence = [];
  const seenDirect = new Set();
  const seenEvidence = new Set();

  for (const interaction of state.interactions) {
    const visible = !viewer
      || interaction.participants.includes(viewer.id)
      || (interaction.attestation !== undefined
        && viewer.knowledge.has(interaction.attestation));
    if (!visible) continue;

    const pairs = interaction.primary
      ? [interaction.primary]
      : interaction.participants.slice(1).map((id) => [interaction.participants[0], id]);
    for (const [from, to] of pairs) {
      const key = `${interaction.kind}:${from}:${to}`;
      if (seenDirect.has(key)) continue;
      seenDirect.add(key);
      direct.push({
        from, to, kind: interaction.kind, outcome: interaction.outcome,
        lane: direct.length % 5 - 2
      });
    }

    if (!viewer || !interaction.target) continue;
    const decisionMaker = interaction.decisionMaker
      ? state.byId.get(interaction.decisionMaker)
      : viewer;
    if (decisionMaker !== viewer) continue;

    for (const idx of viewer.knowledge) {
      const att = state.attestations[idx];
      if (att.target !== interaction.target || !state.byId.has(att.by)) continue;
      const key = `${att.by}:${att.target}:${evidenceClass(att)}`;
      if (seenEvidence.has(key)) continue;
      seenEvidence.add(key);
      evidence.push({
        from: att.by,
        to: att.target,
        class: evidenceClass(att),
        depth: 1,
        counted: p.vouchMode
          ? ["deal-record/1", "objection/1"].includes(att.type)
          : ["deal-record/1", "endorsement/1", "objection/1", "ceremony-record/1"].includes(att.type),
        lane: evidence.length % 7 - 3
      });
    }
  }
  return { direct, evidence };
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
