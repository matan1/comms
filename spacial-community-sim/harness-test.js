// Headless harness for the cost-field / presence-model rework.
// Concatenated after world.js + sim.js + render.js + ui.js (same convention
// as v1: shared scope, no DOM).

function tally() {
  return { trips: 0, days: 0 };
}

function runScenario(rawParams, days, hooks = {}) {
  const p = normalizeParams(rawParams);
  seedState(p);
  if (hooks.afterSeed) hooks.afterSeed(p);
  for (let d = 0; d < days; d += 1) {
    for (let ph = 0; ph < PHASES.length; ph += 1) {
      nextPhase(p);
      if (state.phase === 1 && hooks.afterMarket) hooks.afterMarket(p);
      if (hooks.afterPhase) hooks.afterPhase(p);
    }
  }
  return p;
}

function groupCoverage(group) {
  let firstRecent = state.attestations.length;
  for (let i = state.attestations.length - 1; i >= 0; i -= 1) {
    if (state.attestations[i].day < state.day - 2) break;
    firstRecent = i;
  }
  const recent = state.attestations.length - firstRecent;
  if (!recent || !group.length) return 1;
  let cov = 0;
  for (const v of group) {
    let known = 0;
    for (let i = firstRecent; i < state.attestations.length; i += 1) {
      if (v.knowledge.has(i)) known += 1;
    }
    cov += known / recent;
  }
  return cov / group.length;
}

// ---------- 1. Determinism ----------
function beliefFingerprint() {
  let sum = 0;
  for (const v of state.villagers) {
    for (const [, b] of v.beliefs) sum += b.pos * 31 + b.neg * 17;
  }
  return `${state.attestations.length}|${state.villagers.length}|${sum.toFixed(3)}`;
}

runScenario({}, 40);
const fpA = beliefFingerprint();
runScenario({}, 40);
const fpB = beliefFingerprint();
console.log("1. DETERMINISM:", fpA === fpB ? "PASS" : "FAIL", `(${fpA})`);

// ---------- 2. Emergent attendance + lag ----------
const att = { farm: tally(), nearVill: tally(), farVill: tally() };
let covSamples = { farm: 0, vill: 0, n: 0 };

runScenario({}, 120, {
  afterMarket() {
    for (const v of members()) {
      const bucket = v.farmstead
        ? att.farm
        : (travelCost(v.home, world.market) > 0.22 ? att.farVill : att.nearVill);
      bucket.days += 1;
      if (v.atMarket) bucket.trips += 1;
    }
  },
  afterPhase() {
    if (state.phase === 3) {
      covSamples.farm += groupCoverage(members().filter((v) => v.farmstead));
      covSamples.vill += groupCoverage(members().filter((v) => !v.farmstead));
      covSamples.n += 1;
    }
  }
});

const rate = (t) => (t.days ? (t.trips / t.days) : 0);
console.log("2. ATTENDANCE  near-village:", rate(att.nearVill).toFixed(2),
  "| far-village:", rate(att.farVill).toFixed(2),
  "| farmstead:", rate(att.farm).toFixed(2));
console.log("   COVERAGE    village:", (covSamples.vill / covSamples.n).toFixed(2),
  "| farmstead:", (covSamples.farm / covSamples.n).toFixed(2),
  (covSamples.farm < covSamples.vill ? "-> farm lags: PASS" : "-> FAIL"));

// ---------- 3. Housing growth (the day-566 blob) ----------
runScenario({ arrivalRate: 70 }, 500);
const memberHomes = members().map((v) => `${v.home.x.toFixed(4)},${v.home.y.toFixed(4)}`);
const uniqueHomes = new Set(memberHomes);
const founded = 40;
console.log("3. HOUSING:", members().length, "members,",
  uniqueHomes.size, "distinct homes,",
  world.homes.length + world.farmHomes.length, "total plots",
  uniqueHomes.size === memberHomes.length ? "-> no blob: PASS" : "-> FAIL (shared homes!)");

// New plots should have sane commutes (settled near roads, not exiled).
const newPlots = world.homes.slice(founded).filter((h) => h.claimed);
if (newPlots.length) {
  const costs = newPlots.map((h) => travelCost(h, world.market));
  console.log("   new plots:", newPlots.length,
    "| commute min/mean/max:",
    Math.min(...costs).toFixed(2),
    (costs.reduce((a, b) => a + b, 0) / costs.length).toFixed(2),
    Math.max(...costs).toFixed(2));
}

// ---------- 4. Defector arc ----------
let defectorId = null;
runScenario({}, 50, {
  afterSeed() {
    const d = arrive("defector");
    defectorId = d.id;
  }
});
const def = state.byId.get(defectorId);
if (!def) {
  console.log("4. DEFECTOR: moved on before admission (acceptable but check pacing)");
} else {
  const villageView = [];
  const farmView = [];
  for (const v of members()) {
    if (v.id === defectorId) continue;
    (v.farmstead ? farmView : villageView).push(perceivedTrust(v, defectorId));
  }
  const mean = (a) => a.reduce((x, y) => x + y, 0) / Math.max(1, a.length);
  console.log("4. DEFECTOR:", def.member ? `admitted day ${def.joinedDay}` : "never admitted",
    "| village view:", mean(villageView).toFixed(2),
    "| farmstead view:", mean(farmView).toFixed(2),
    def.member && mean(villageView) < 0.45 ? "-> scandal landed: PASS" : "(inspect)");
}

// ---------- 5. Cost API sanity ----------
seedState(normalizeParams({}));
const farmHome = world.farmHomes[0];
const direct = dist(farmHome, world.market);
const walk = travelCost(farmHome, world.market);
const carted = travelCost(farmHome, world.market, { cart: true });
const msg = messagingCost(farmHome, world.market);
console.log("5. COST API: euclid", direct.toFixed(2),
  "| walk", walk.toFixed(2),
  "| cart", carted.toFixed(2),
  "| messaging", msg.toFixed(2),
  walk < direct && carted < walk ? "-> road discount works: PASS" : "-> FAIL");
let threw = false;
try { cost("teleport", farmHome, world.market); } catch (e) { threw = true; }
console.log("   unknown op throws:", threw ? "PASS" : "FAIL");

// ---------- 6. Timing ----------
const t0 = Date.now();
runScenario({}, 200);
console.log("6. TIMING: 200 days in", Date.now() - t0, "ms");
