const names = [
  "Ari", "Bex", "Cato", "Dara", "Eli", "Faye", "Galen", "Hana", "Ivo",
  "Jules", "Kira", "Lena", "Mika", "Nia", "Oren", "Pax", "Quin", "Rhea",
  "Sami", "Tala", "Uma", "Vale", "Wren", "Xan", "Yara", "Zev", "Anik",
  "Bria", "Corin", "Dev", "Eno", "Fia", "Grey", "Hale", "Iris", "Joss",
  "Kael", "Liv", "Maro", "Noor", "Ola", "Pim", "Remy", "Sol"
];

const goods = [
  { id: "grain", label: "Grain", basePrice: 1.0, baseDemand: 2.6 },
  { id: "tools", label: "Tools", basePrice: 2.8, baseDemand: 0.55 },
  { id: "care", label: "Care", basePrice: 2.1, baseDemand: 0.9 },
  { id: "data", label: "Data", basePrice: 1.7, baseDemand: 1.25 }
];

const controls = {
  population: byId("population"),
  trust: byId("trust"),
  attestFreq: byId("attestFreq"),
  objectionRate: byId("objectionRate"),
  sponsors: byId("sponsors"),
  witnessQuorum: byId("witnessQuorum"),
  commitRule: byId("commitRule"),
  pool: byId("pool"),
  seedFloor: byId("seedFloor"),
  cap: byId("cap"),
  returnRate: byId("returnRate"),
  priceMode: byId("priceMode"),
  priceSignal: byId("priceSignal"),
  productionVariance: byId("productionVariance"),
  runSpeed: byId("runSpeed")
};

const outputs = {
  population: byId("populationOut"),
  trust: byId("trustOut"),
  attestFreq: byId("attestFreqOut"),
  objectionRate: byId("objectionRateOut"),
  sponsors: byId("sponsorsOut"),
  witnessQuorum: byId("witnessQuorumOut"),
  pool: byId("poolOut"),
  seedFloor: byId("seedFloorOut"),
  cap: byId("capOut"),
  returnRate: byId("returnRateOut"),
  priceSignal: byId("priceSignalOut"),
  productionVariance: byId("productionVarianceOut"),
  runSpeed: byId("runSpeedOut")
};

const ui = {
  stepBtn: byId("stepBtn"),
  runBtn: byId("runBtn"),
  resetBtn: byId("resetBtn"),
  exportBtn: byId("exportBtn"),
  networkCanvas: byId("networkCanvas"),
  auditCanvas: byId("auditCanvas"),
  eventLog: byId("eventLog"),
  allocationTable: byId("allocationTable"),
  marketTable: byId("marketTable"),
  cycleMetric: byId("cycleMetric"),
  membersMetric: byId("membersMetric"),
  trustMetric: byId("trustMetric"),
  attestMetric: byId("attestMetric"),
  giniMetric: byId("giniMetric"),
  ceremonyCount: byId("ceremonyCount"),
  endorsementCount: byId("endorsementCount"),
  objectionCount: byId("objectionCount"),
  commitStatus: byId("commitStatus"),
  poolLabel: byId("poolLabel"),
  marketLabel: byId("marketLabel")
};

let state;
let timer = null;
let fallbackSeed = 9137;

function byId(id) {
  return document.getElementById(id);
}

// Raw slider/select values, in the same units the DOM controls use.
function rawFromControls() {
  return {
    population: controls.population.value,
    trust: controls.trust.value,
    attestFreq: controls.attestFreq.value,
    objectionRate: controls.objectionRate.value,
    sponsors: controls.sponsors.value,
    witnessQuorum: controls.witnessQuorum.value,
    commitRule: controls.commitRule.value,
    pool: controls.pool.value,
    seedFloor: controls.seedFloor.value,
    cap: controls.cap.value,
    returnRate: controls.returnRate.value,
    priceMode: controls.priceMode.value,
    priceSignal: controls.priceSignal.value,
    productionVariance: controls.productionVariance.value,
    runSpeed: controls.runSpeed.value
  };
}

// Convert raw control values into the normalized params the simulation runs on.
function normalizeParams(raw) {
  return {
    population: Number(raw.population),
    trust: Number(raw.trust) / 100,
    attestFreq: Number(raw.attestFreq),
    objectionRate: Number(raw.objectionRate) / 100,
    sponsors: Number(raw.sponsors),
    witnessQuorum: Number(raw.witnessQuorum),
    commitRule: raw.commitRule,
    pool: Number(raw.pool),
    seedFloor: Number(raw.seedFloor) / 100,
    cap: Number(raw.cap) / 100,
    returnRate: Number(raw.returnRate) / 100,
    priceMode: raw.priceMode,
    priceSignal: Number(raw.priceSignal) / 100,
    productionVariance: Number(raw.productionVariance) / 100,
    runSpeed: Number(raw.runSpeed)
  };
}

function params() {
  return normalizeParams(rawFromControls());
}

function syncOutputs() {
  const p = params();
  outputs.population.value = p.population;
  outputs.trust.value = `${Math.round(p.trust * 100)}%`;
  outputs.attestFreq.value = `${p.attestFreq}/cycle`;
  outputs.objectionRate.value = `${Math.round(p.objectionRate * 100)}%`;
  outputs.sponsors.value = p.sponsors;
  outputs.witnessQuorum.value = p.witnessQuorum;
  outputs.pool.value = p.pool;
  outputs.seedFloor.value = `${Math.round(p.seedFloor * 100)}%`;
  outputs.cap.value = `${Math.round(p.cap * 100)}%`;
  outputs.returnRate.value = `${Math.round(p.returnRate * 100)}%`;
  outputs.priceSignal.value = `${Math.round(p.priceSignal * 100)}%`;
  outputs.productionVariance.value = `${Math.round(p.productionVariance * 100)}%`;
  outputs.runSpeed.value = `${runDelay(p.runSpeed)} ms`;
}

function runDelay(speed) {
  return Math.round(1100 - speed * 95);
}

// Build a fresh simulation state into the module-level `state` for params `p`.
// Does not touch the DOM, so it is safe for both live reset and headless tuning.
function seedState(p) {
  state = {
    cycle: 0,
    seed: 1337 + p.population * 17 + Math.round(p.trust * 100),
    stewards: [],
    market: initMarket(),
    attestations: [],
    events: [],
    lastAllocation: [],
    nextId: 1
  };

  for (let i = 0; i < p.population; i += 1) {
    state.stewards.push(makeSteward(i, p.trust, true));
  }

  for (let i = 0; i < 5; i += 1) {
    state.stewards.push(makeSteward(p.population + i, p.trust * 0.72, false));
  }

  logEvent("rule/1", "Founding rule loaded", "Baseline trust, ceremony rule, and resource economy initialized.");
  allocateResources(p);
  runGoodsEconomy(p, { quiet: true, recordSignals: false });
}

function reset() {
  seedState(params());
  render();
}

function initMarket() {
  return {
    goods: Object.fromEntries(goods.map((g) => [g.id, {
      ...g,
      price: g.basePrice,
      supply: 0,
      demand: 0,
      cleared: 0,
      attestations: 0,
      pressure: 0
    }])),
    volume: 0,
    unmet: 0
  };
}

function makeSteward(index, baselineTrust, member) {
  const angle = (Math.PI * 2 * index) / Math.max(1, Number(controls.population.value));
  const jitter = rand() * 0.3;
  return {
    id: `comms.steward:z${String(index + 1).padStart(3, "0")}`,
    label: names[index % names.length],
    member,
    candidate: !member,
    sponsors: [],
    trust: clamp(baselineTrust + (rand() - 0.5) * 0.28, 0.05, 0.97),
    trustPrior: clamp(baselineTrust + (rand() - 0.5) * 0.16, 0.05, 0.97),
    dealStats: { completed: 0, failed: 0 },
    capability: clamp(0.35 + rand() * 0.75, 0.1, 1.15),
    need: Math.round(8 + rand() * 72),
    resources: 0,
    credits: 0,
    satisfaction: 1,
    specialty: goods[index % goods.length].id,
    inventory: Object.fromEntries(goods.map((g) => [g.id, 0])),
    demandBias: Object.fromEntries(goods.map((g) => [g.id, 0.75 + rand() * 0.65])),
    x: 0.5 + Math.cos(angle + jitter) * (member ? 0.33 : 0.42),
    y: 0.5 + Math.sin(angle + jitter) * (member ? 0.31 : 0.38)
  };
}

function rand() {
  const seed = state ? state.seed++ : fallbackSeed++;
  const x = Math.sin(seed) * 10000;
  return x - Math.floor(x);
}

// One simulation cycle with no rendering or DOM access.
function advanceCycle(p) {
  state.cycle += 1;
  driftConditions(p);
  runCeremonyCycle(p);
  produceAttestations(p);
  allocateResources(p);
  runGoodsEconomy(p);
  maybeReturnResources(p);
}

function step() {
  advanceCycle(params());
  render();
}

function driftConditions(p) {
  for (const s of state.stewards) {
    s.need = clamp(s.need + Math.round((rand() - 0.42) * 10), 3, 96);
  }
}

function recomputeTrust(steward) {
  const completed = steward.dealStats.completed;
  const failed = steward.dealStats.failed;
  const priorWeight = 8;
  const total = completed + failed + priorWeight;
  steward.trust = clamp((completed + steward.trustPrior * priorWeight) / total, 0.02, 0.99);
}

function noteDeal(steward, completed, failed = 0) {
  steward.dealStats.completed += completed;
  steward.dealStats.failed += failed;
  recomputeTrust(steward);
}

function runCeremonyCycle(p) {
  const candidates = state.stewards.filter((s) => !s.member);
  if (!candidates.length && rand() < 0.35) {
    state.stewards.push(makeSteward(state.stewards.length, p.trust * 0.7, false));
  }

  const candidate = pick(state.stewards.filter((s) => !s.member));
  if (!candidate) {
    return;
  }

  const sponsors = activeMembers()
    .filter((s) => s.trust > 0.38 && rand() < s.trust * candidate.trust)
    .sort((a, b) => b.trust - a.trust)
    .slice(0, Math.max(p.sponsors, 8));

  const witnesses = activeMembers()
    .filter((s) => rand() < 0.3 + s.trust * 0.45)
    .slice(0, 14);

  const objections = witnesses.filter((s) => rand() < p.objectionRate * (1.1 - candidate.trust));
  candidate.sponsors = sponsors.slice(0, p.sponsors).map((s) => s.id);

  const committed = ceremonyCommitted(p, sponsors, witnesses, objections, candidate);
  recordAttestation("ceremony-record/1", committed ? "committed" : "held", {
    subject: candidate.id,
    sponsors: sponsors.length,
    witnesses: witnesses.length,
    objections: objections.length
  });

  if (committed) {
    candidate.member = true;
    candidate.candidate = false;
    noteDeal(candidate, 2 + sponsors.length, 0);
    for (const s of sponsors.slice(0, p.sponsors)) {
      noteDeal(s, 1, 0);
      recordAttestation("endorsement/1", "sponsor", { target: candidate.id, by: s.id });
    }
    logEvent("ceremony-record/1", `${candidate.label} admitted`, `${sponsors.length} sponsors, ${witnesses.length} witnesses, ${objections.length} objections.`);
  } else {
    noteDeal(candidate, 0, 1 + objections.length);
    for (const o of objections) {
      recordAttestation("objection/1", "procedural", { target: candidate.id, by: o.id });
    }
    logEvent("objection/1", `${candidate.label} not admitted`, explainFailure(p, sponsors, witnesses, objections));
  }
}

function ceremonyCommitted(p, sponsors, witnesses, objections, candidate) {
  if (sponsors.length < p.sponsors || witnesses.length < p.witnessQuorum) {
    return false;
  }
  if (p.commitRule === "majority") {
    const consent = witnesses.length - objections.length + sponsors.length;
    return consent > activeMembers().length / 2 && candidate.trust > 0.25;
  }
  if (p.commitRule === "unanimous-witnesses") {
    return objections.length === 0 && witnesses.length >= p.witnessQuorum;
  }
  return sponsors.length >= p.sponsors && objections.length <= Math.floor(witnesses.length * 0.25);
}

function explainFailure(p, sponsors, witnesses, objections) {
  if (sponsors.length < p.sponsors) {
    return `Only ${sponsors.length} sponsors; rule requires ${p.sponsors}.`;
  }
  if (witnesses.length < p.witnessQuorum) {
    return `Only ${witnesses.length} witnesses; quorum is ${p.witnessQuorum}.`;
  }
  return `${objections.length} objections prevented commitment under ${p.commitRule}.`;
}

function produceAttestations(p) {
  const members = activeMembers();
  const count = Math.min(p.attestFreq, members.length);
  for (let i = 0; i < count; i += 1) {
    const steward = pick(members);
    if (!steward) {
      continue;
    }
    const kind = rand() < 0.22 ? "recognition/1" : "general-claim/1";
    const peer = pick(members.filter((s) => s.id !== steward.id));
    recordAttestation(kind, kind === "recognition/1" ? "mutual-recognition" : "observation", {
      by: steward.id,
      target: peer?.id
    });
  }
}

function runGoodsEconomy(p, options = {}) {
  const members = activeMembers();
  const marketRows = Object.values(state.market.goods);
  for (const good of marketRows) {
    good.supply = 0;
    good.demand = 0;
    good.cleared = 0;
    good.attestations = 0;
    good.pressure = 0;
  }
  state.market.volume = 0;
  state.market.unmet = 0;

  for (const steward of members) {
    steward.credits = steward.resources;
    for (const key of Object.keys(steward.inventory)) {
      steward.inventory[key] *= 0.25;
    }
    const produced = state.market.goods[steward.specialty];
    const variance = 1 + (rand() - 0.5) * p.productionVariance;
    const amount = Math.max(0, (0.8 + steward.capability * 2.4) * (0.65 + steward.trust) * variance);
    steward.inventory[produced.id] += amount;
    produced.supply += amount;

    for (const good of marketRows) {
      const demand = good.baseDemand * steward.demandBias[good.id] * (0.78 + (1 - steward.trust) * 0.35);
      good.demand += demand;
    }
  }

  attestPrices(p, members, options);
  updatePrices(p);

  for (const buyer of members) {
    const basket = goods
      .map((good) => {
        const marketGood = state.market.goods[good.id];
        const have = buyer.inventory[good.id] || 0;
        const want = Math.max(0, good.baseDemand * buyer.demandBias[good.id] - have);
        return { good: marketGood, want, priority: want / Math.max(0.2, marketGood.price) };
      })
      .sort((a, b) => b.priority - a.priority);

    let filled = 0;
    let wanted = 0;
    let spent = 0;
    let primaryGood = null;
    let failedTerms = 0;
    for (const item of basket) {
      wanted += item.want;
      if (item.want <= 0 || buyer.credits <= 0) {
        if (item.want > 0) {
          failedTerms += 1;
        }
        continue;
      }
      const available = Math.max(0, item.good.supply - item.good.cleared);
      const affordable = buyer.credits / item.good.price;
      const quantity = Math.min(item.want, available, affordable);
      if (quantity <= 0) {
        failedTerms += 1;
        continue;
      }
      buyer.inventory[item.good.id] = (buyer.inventory[item.good.id] || 0) + quantity;
      const cost = quantity * item.good.price;
      buyer.credits -= cost;
      item.good.cleared += quantity;
      state.market.volume += cost;
      filled += quantity;
      spent += cost;
      primaryGood = primaryGood || item.good.id;
    }
    buyer.satisfaction = wanted > 0 ? clamp(filled / wanted, 0, 1) : 1;
    const completedDeals = Math.floor(filled);
    const unmetDeals = failedTerms + Math.floor(Math.max(0, wanted - filled));
    noteDeal(buyer, completedDeals, unmetDeals);
    if (options.recordSignals !== false && spent > 0 && rand() < 0.08 + p.priceSignal * 0.12) {
      recordAttestation("purchase-decision/1", "buy", {
        by: buyer.id,
        primary_good: primaryGood,
        spent: Number(spent.toFixed(2)),
        satisfaction: Number(buyer.satisfaction.toFixed(2))
      });
    }
  }

  state.market.unmet = marketRows.reduce((sum, good) => sum + Math.max(0, good.demand - good.cleared), 0);
  if (state.cycle > 0 && !options.quiet) {
    const scarce = [...marketRows].sort((a, b) => (b.demand - b.cleared) - (a.demand - a.cleared))[0];
    recordAttestation("market-clearing/1", "decision", {
      mode: p.priceMode,
      volume: Number(state.market.volume.toFixed(2)),
      unmet: Number(state.market.unmet.toFixed(2)),
      prices: Object.fromEntries(marketRows.map((good) => [good.id, Number(good.price.toFixed(2))]))
    });
    logEvent("market-clearing/1", "Market cleared", `${state.market.volume.toFixed(1)} credits traded; ${scarce.label} had the largest unmet demand.`);
  }
}

function attestPrices(p, members, options = {}) {
  if (!members.length) {
    return;
  }
  const witnesses = members
    .filter((s) => rand() < 0.25 + p.priceSignal * s.trust * 0.55)
    .slice(0, Math.max(2, Math.round(members.length * 0.35)));

  for (const good of Object.values(state.market.goods)) {
    let weighted = 0;
    let weight = 0;
    const quotes = [];
    for (const steward of witnesses) {
      const scarcity = (good.demand + 0.1) / (good.supply + 0.1);
      const sellerBias = steward.specialty === good.id ? 0.18 : 0;
      const buyerBias = (steward.demandBias[good.id] - 1) * -0.08;
      const quote = good.price * clamp(1 + Math.log(scarcity) * 0.22 + sellerBias + buyerBias + (rand() - 0.5) * 0.18, 0.35, 2.8);
      weighted += quote * steward.trust;
      weight += steward.trust;
      good.attestations += 1;
      quotes.push({ steward, quote });
    }
    const attestedPrice = weight > 0 ? weighted / weight : good.price;
    good.pressure = weight > 0 ? (attestedPrice - good.price) / good.price : 0;
    if (options.recordSignals !== false) {
      for (const item of quotes) {
        if (rand() >= p.priceSignal * 0.35) {
          continue;
        }
        if (Math.abs(item.quote - attestedPrice) / Math.max(0.1, attestedPrice) < 0.35) {
          noteDeal(item.steward, 1, 0);
        } else {
          noteDeal(item.steward, 0, 1);
        }
        recordAttestation("price-signal/1", "quote", {
          by: item.steward.id,
          good: good.id,
          quote: Number(item.quote.toFixed(2))
        });
      }
    }
  }
}

function updatePrices(p) {
  for (const good of Object.values(state.market.goods)) {
    if (p.priceMode === "fixed") {
      good.price = good.basePrice;
      continue;
    }
    const scarcity = (good.demand + 0.2) / (good.supply + 0.2);
    const marketPressure = clamp(Math.log(scarcity) * 0.2, -0.28, 0.38);
    const attestedPressure = clamp(good.pressure, -0.35, 0.45) * p.priceSignal;
    good.price = clamp(good.price * (1 + marketPressure + attestedPressure), good.basePrice * 0.25, good.basePrice * 5.5);
  }
}

function allocateResources(p = params()) {
  const members = activeMembers();
  if (!members.length) {
    state.lastAllocation = [];
    return;
  }

  const seedPool = p.pool * p.seedFloor;
  const seedEach = seedPool / members.length;
  const meritPool = p.pool - seedPool;
  const scores = new Map();

  for (const s of members) {
    const sponsorBoost = 1 + Math.min(4, s.sponsors.length) * 0.16;
    const score = Math.max(0, s.need - seedEach) * (0.55 + s.capability) * (0.35 + s.trust) * sponsorBoost;
    scores.set(s.id, score);
  }

  const totalScore = Array.from(scores.values()).reduce((a, b) => a + b, 0);
  const meritCap = meritPool * p.cap;
  const grants = new Map(members.map((s) => [s.id, seedEach]));

  if (totalScore > 0) {
    let overflow = 0;
    const uncapped = [];
    for (const s of members) {
      const raw = meritPool * (scores.get(s.id) / totalScore);
      const capped = Math.min(raw, meritCap);
      grants.set(s.id, grants.get(s.id) + capped);
      overflow += raw - capped;
      if (raw < meritCap) {
        uncapped.push(s);
      }
    }
    const uncappedScore = uncapped.reduce((sum, s) => sum + scores.get(s.id), 0);
    for (const s of uncapped) {
      const add = uncappedScore > 0 ? overflow * (scores.get(s.id) / uncappedScore) : 0;
      grants.set(s.id, Math.min(grants.get(s.id) + add, seedEach + meritCap));
    }
  }

  state.lastAllocation = members
    .map((s) => {
      const grant = grants.get(s.id);
      s.resources = grant;
      return { steward: s, grant, score: scores.get(s.id) };
    })
    .sort((a, b) => b.grant - a.grant);
}

function maybeReturnResources(p) {
  const wealthy = state.lastAllocation.filter((g) => g.grant > g.steward.need * 0.85 && g.steward.trust > 0.55);
  const giver = pick(wealthy);
  if (!giver || rand() > p.returnRate) {
    return;
  }
  const amount = giver.grant * (0.05 + rand() * 0.18);
  giver.steward.resources -= amount;
  recordAttestation("allocation-return/1", "returned", { by: giver.steward.id, amount: amount.toFixed(2) });
  logEvent("allocation-return/1", `${giver.steward.label} returned resources`, `${amount.toFixed(1)} units returned to the commons for the next allocation.`);
}

function recordAttestation(type, role, detail) {
  state.attestations.push({
    id: `comms.attest:z${String(state.nextId++).padStart(5, "0")}`,
    cycle: state.cycle,
    type,
    role,
    detail
  });
}

function logEvent(type, title, body) {
  state.events.unshift({ cycle: state.cycle, type, title, body });
  state.events = state.events.slice(0, 80);
}

function activeMembers() {
  return state.stewards.filter((s) => s.member);
}

function pick(items) {
  if (!items.length) {
    return null;
  }
  return items[Math.floor(rand() * items.length)];
}

function average(values) {
  return values.length ? values.reduce((a, b) => a + b, 0) / values.length : 0;
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function gini(values) {
  if (!values.length) {
    return 0;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const sum = sorted.reduce((a, b) => a + b, 0);
  if (sum === 0) {
    return 0;
  }
  let weighted = 0;
  for (let i = 0; i < sorted.length; i += 1) {
    weighted += (i + 1) * sorted[i];
  }
  return (2 * weighted) / (sorted.length * sum) - (sorted.length + 1) / sorted.length;
}

function render() {
  const members = activeMembers();
  ui.cycleMetric.textContent = state.cycle;
  ui.membersMetric.textContent = members.length;
  ui.trustMetric.textContent = `${Math.round(average(members.map((s) => s.trust)) * 100)}%`;
  ui.attestMetric.textContent = state.attestations.length;
  ui.giniMetric.textContent = gini(state.lastAllocation.map((g) => g.grant)).toFixed(2);
  ui.ceremonyCount.textContent = countType("ceremony-record/1");
  ui.endorsementCount.textContent = countType("endorsement/1");
  ui.objectionCount.textContent = countType("objection/1");
  ui.commitStatus.textContent = statusText();
  ui.poolLabel.textContent = `${params().pool} units`;
  ui.marketLabel.textContent = `${params().priceMode}; ${state.market.volume.toFixed(0)} traded`;
  renderAllocation();
  renderMarket();
  renderEvents();
  drawNetwork();
  drawAudit();
}

function countType(type) {
  return state.attestations.filter((a) => a.type === type).length;
}

function statusText() {
  const last = state.events.find((e) => e.type === "ceremony-record/1" || e.type === "objection/1");
  return last ? last.title : "ready";
}

function renderAllocation() {
  ui.allocationTable.innerHTML = "";
  for (const row of state.lastAllocation.slice(0, 14)) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td>${row.steward.label}</td>
      <td>${Math.round(row.steward.need)}</td>
      <td>${Math.round(row.steward.trust * 100)}%</td>
      <td>${row.grant.toFixed(1)}</td>
    `;
    ui.allocationTable.appendChild(tr);
  }
}

function renderMarket() {
  ui.marketTable.innerHTML = "";
  for (const good of Object.values(state.market.goods)) {
    const tr = document.createElement("tr");
    const pressureClass = good.pressure > 0.05 ? "up" : good.pressure < -0.05 ? "down" : "";
    tr.innerHTML = `
      <td>${good.label}</td>
      <td>${good.supply.toFixed(1)}</td>
      <td>${good.demand.toFixed(1)}</td>
      <td class="${pressureClass}">${good.price.toFixed(2)}</td>
      <td>${good.cleared.toFixed(1)}</td>
    `;
    ui.marketTable.appendChild(tr);
  }
}

function renderEvents() {
  ui.eventLog.innerHTML = "";
  for (const event of state.events.slice(0, 24)) {
    const li = document.createElement("li");
    li.innerHTML = `
      <time>cycle ${event.cycle}</time>
      <div><b>${event.title}</b><p>${event.body}</p></div>
    `;
    ui.eventLog.appendChild(li);
  }
}

function drawNetwork() {
  const canvas = ui.networkCanvas;
  const ctx = scaledContext(canvas);
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = "#fbfcf8";
  ctx.fillRect(0, 0, w, h);

  const members = activeMembers();
  ctx.lineWidth = 1;
  for (const s of state.stewards) {
    if (!s.member) {
      continue;
    }
    const sponsors = members.filter((m) => m.id !== s.id && randForPair(s.id, m.id) < 0.08 + s.trust * m.trust * 0.07);
    for (const t of sponsors.slice(0, 3)) {
      ctx.strokeStyle = `rgba(46, 111, 158, ${0.08 + Math.min(s.trust, t.trust) * 0.22})`;
      ctx.beginPath();
      ctx.moveTo(s.x * w, s.y * h);
      ctx.lineTo(t.x * w, t.y * h);
      ctx.stroke();
    }
  }

  for (const s of state.stewards) {
    const x = s.x * w;
    const y = s.y * h;
    const radius = 6 + s.capability * 7;
    ctx.beginPath();
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.fillStyle = s.member ? colorForTrust(s.trust) : "#b87815";
    ctx.fill();
    if (s.sponsors.length) {
      ctx.strokeStyle = "#2e6f9e";
      ctx.lineWidth = 3;
    } else {
      ctx.strokeStyle = "rgba(30, 37, 39, 0.22)";
      ctx.lineWidth = 1;
    }
    ctx.stroke();

    if (radius > 11 || s.trust > 0.72) {
      ctx.fillStyle = "#263033";
      ctx.font = "12px system-ui";
      ctx.fillText(s.label, x + radius + 4, y + 4);
    }
  }
}

function randForPair(a, b) {
  let hash = 0;
  const key = `${a}:${b}:${state.cycle}`;
  for (let i = 0; i < key.length; i += 1) {
    hash = (hash * 31 + key.charCodeAt(i)) >>> 0;
  }
  return (hash % 1000) / 1000;
}

function colorForTrust(value) {
  if (value > 0.72) {
    return "#2f7d66";
  }
  if (value > 0.48) {
    return "#6a8f5a";
  }
  if (value > 0.3) {
    return "#b87815";
  }
  return "#b65345";
}

function drawAudit() {
  const canvas = ui.auditCanvas;
  const ctx = scaledContext(canvas);
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = "#fbfcf8";
  ctx.fillRect(0, 0, w, h);

  const types = [
    ["rule/1", "#6a5c93"],
    ["ceremony-record/1", "#2e6f9e"],
    ["endorsement/1", "#2f7d66"],
    ["objection/1", "#b65345"],
    ["general-claim/1", "#b87815"],
    ["price-signal/1", "#4f8f8b"],
    ["market-clearing/1", "#6a5c93"],
    ["allocation-return/1", "#637074"]
  ];
  const max = Math.max(1, ...types.map(([type]) => countType(type)));
  const barW = w / types.length - 16;
  types.forEach(([type, color], index) => {
    const count = countType(type);
    const bh = (h - 66) * (count / max);
    const x = 12 + index * (barW + 16);
    const y = h - 34 - bh;
    ctx.fillStyle = color;
    ctx.fillRect(x, y, barW, bh);
    ctx.fillStyle = "#263033";
    ctx.font = "12px system-ui";
    ctx.fillText(String(count), x + 2, y - 6);
    ctx.save();
    ctx.translate(x + 2, h - 12);
    ctx.rotate(-0.45);
    ctx.fillStyle = "#637074";
    ctx.fillText(type.replace("/1", ""), 0, 0);
    ctx.restore();
  });
}

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

function exportSummary() {
  const summary = {
    parameters: params(),
    cycle: state.cycle,
    stewards: activeMembers().map((s) => ({
      id: s.id,
      label: s.label,
      trust: Number(s.trust.toFixed(3)),
      resources: Number(s.resources.toFixed(2)),
      specialty: s.specialty,
      satisfaction: Number(s.satisfaction.toFixed(3))
    })),
    market: state.market,
    attestations: state.attestations
  };
  const blob = new Blob([JSON.stringify(summary, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `comms-community-sim-cycle-${state.cycle}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

// --- Auto-Tune -------------------------------------------------------------
// A fully static, browser-side search for balanced simulator parameters.
// Candidate parameter sets are evaluated off-screen (the live simulation is
// never mutated) and ranked against a "Healthy Balance" target. No models,
// training, or network services: the sim is small and deterministic, so a
// direct randomized/grid hybrid search is a better fit than anything heavier.

const tune = {
  budget: byId("tuneBudget"),
  runBtn: byId("tuneBtn"),
  status: byId("tuneStatus"),
  results: byId("tuneResults")
};

const tuneBudgets = {
  quick: { label: "Quick", candidates: 14, horizon: 30, sample: 8 },
  normal: { label: "Normal", candidates: 30, horizon: 42, sample: 12 },
  deep: { label: "Deep", candidates: 64, horizon: 56, sample: 16 }
};

// Conservative ranges (in raw control units) for the parameters we tune.
// Population and run speed are intentionally excluded so candidates stay
// comparable to the user's current scenario.
const tuneSpace = {
  trust: { min: 45, max: 82, step: 1 },
  attestFreq: { min: 3, max: 8, step: 1 },
  objectionRate: { min: 2, max: 22, step: 1 },
  sponsors: { min: 1, max: 4, step: 1 },
  witnessQuorum: { min: 2, max: 6, step: 1 },
  pool: { min: 160, max: 440, step: 10 },
  seedFloor: { min: 12, max: 36, step: 1 },
  cap: { min: 24, max: 52, step: 1 },
  returnRate: { min: 4, max: 26, step: 1 },
  priceSignal: { min: 20, max: 80, step: 5 },
  productionVariance: { min: 8, max: 40, step: 2 }
};
const tuneCommitRules = ["sponsor-quorum", "majority", "unanimous-witnesses"];
const tunePriceModes = ["floating", "fixed"];

// Human-friendly labels for the parameters a candidate may change.
const tuneLabels = {
  trust: "Initial deal prior",
  attestFreq: "Attestation frequency",
  objectionRate: "Objection pressure",
  sponsors: "Minimum sponsors",
  witnessQuorum: "Witness quorum",
  commitRule: "Commit rule",
  pool: "Resource pool",
  seedFloor: "Seed floor",
  cap: "Per-steward cap",
  returnRate: "Return culture",
  priceMode: "Exchange rates",
  priceSignal: "Price attestations",
  productionVariance: "Production variance"
};

let tuning = false;

// Small deterministic RNG (mulberry32) so a given budget/scenario always
// produces the same candidate set and ranking.
function makeRng(seed) {
  let s = seed >>> 0;
  return function rng() {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function sampleRange(rng, range) {
  const raw = range.min + rng() * (range.max - range.min);
  const stepped = Math.round(raw / range.step) * range.step;
  return clamp(stepped, range.min, range.max);
}

function midRange(range) {
  return clamp(Math.round((range.min + range.max) / 2 / range.step) * range.step, range.min, range.max);
}

// Randomized/grid hybrid: the current settings plus a grid of balanced anchors
// over the categorical controls, then randomized fill across the ranges.
function makeCandidates(budget) {
  const rng = makeRng(0x51eed);
  const base = rawFromControls();
  const list = [{ ...base }];

  for (const commitRule of tuneCommitRules) {
    for (const priceMode of tunePriceModes) {
      const raw = { ...base, commitRule, priceMode };
      for (const key of Object.keys(tuneSpace)) {
        raw[key] = midRange(tuneSpace[key]);
      }
      list.push(raw);
    }
  }

  while (list.length < budget.candidates) {
    const raw = { ...base };
    for (const key of Object.keys(tuneSpace)) {
      raw[key] = sampleRange(rng, tuneSpace[key]);
    }
    raw.commitRule = tuneCommitRules[Math.floor(rng() * tuneCommitRules.length)];
    raw.priceMode = tunePriceModes[Math.floor(rng() * tunePriceModes.length)];
    list.push(raw);
  }

  return list.slice(0, budget.candidates);
}

function snapshotMetrics() {
  const members = activeMembers();
  const totalDemand = Object.values(state.market.goods).reduce((sum, g) => sum + g.demand, 0);
  return {
    trust: average(members.map((s) => s.trust)),
    satisfaction: average(members.map((s) => s.satisfaction)),
    unmetFraction: totalDemand > 0 ? clamp(state.market.unmet / totalDemand, 0, 1) : 0,
    gini: gini(state.lastAllocation.map((g) => g.grant)),
    members: members.length
  };
}

function aggregateSamples(samples) {
  const keys = ["trust", "satisfaction", "unmetFraction", "gini", "members"];
  const out = {};
  for (const key of keys) {
    out[key] = average(samples.map((s) => s[key]));
  }
  return out;
}

// Run one candidate off-screen and return its averaged recent-cycle metrics.
// The live `state` is saved and restored, so the visible sim is untouched.
function evaluateRaw(raw, budget) {
  const p = normalizeParams(raw);
  const saved = state;
  try {
    seedState(p);
    const samples = [];
    for (let c = 0; c < budget.horizon; c += 1) {
      advanceCycle(p);
      if (c >= budget.horizon - budget.sample) {
        samples.push(snapshotMetrics());
      }
    }
    const metrics = aggregateSamples(samples);
    metrics.committed = state.attestations.filter((a) => a.type === "ceremony-record/1" && a.role === "committed").length;
    metrics.held = state.attestations.filter((a) => a.type === "ceremony-record/1" && a.role === "held").length;
    metrics.objections = state.attestations.filter((a) => a.type === "objection/1").length;
    return metrics;
  } finally {
    state = saved;
  }
}

// "Healthy Balance" objective: high deal trust and satisfaction, low unmet
// demand, moderate inequality, and continued ceremony/admission activity.
function scoreMetrics(m, budget) {
  const trust = clamp(m.trust, 0, 1);
  const satisfaction = clamp(m.satisfaction, 0, 1);
  const coverage = clamp(1 - m.unmetFraction, 0, 1);
  const giniHealth = clamp(1 - Math.abs(m.gini - 0.3) / 0.42, 0, 1);
  const totalCeremonies = m.committed + m.held;
  const admissionRate = totalCeremonies > 0 ? m.committed / totalCeremonies : 0;
  const activity = clamp(totalCeremonies / Math.max(1, budget.horizon * 0.5), 0, 1);
  const admissionHealth = totalCeremonies > 0 ? 0.55 * admissionRate + 0.45 * activity : 0.2;

  const parts = {
    trust: 0.3 * trust,
    satisfaction: 0.26 * satisfaction,
    coverage: 0.18 * coverage,
    gini: 0.16 * giniHealth,
    admission: 0.1 * admissionHealth
  };
  const score = parts.trust + parts.satisfaction + parts.coverage + parts.gini + parts.admission;
  return { score, parts, trust, satisfaction, coverage, giniHealth, admissionRate, admissionHealth, totalCeremonies };
}

function tunePct(value) {
  return `${Math.round(value * 100)}%`;
}

// Explain why a candidate ranked where it did, by its strongest and weakest
// normalized components.
function explainCandidate(sc) {
  const comps = [
    { key: "deal trust", v: sc.trust },
    { key: "market satisfaction", v: sc.satisfaction },
    { key: "demand coverage", v: sc.coverage },
    { key: "balanced inequality", v: sc.giniHealth },
    { key: "admission activity", v: sc.admissionHealth }
  ];
  const sorted = [...comps].sort((a, b) => b.v - a.v);
  const best = sorted[0];
  const worst = sorted[sorted.length - 1];
  return `Strong ${best.key} (${tunePct(best.v)}); weakest on ${worst.key} (${tunePct(worst.v)}).`;
}

// Tuned fields whose value differs from the user's current settings.
function changedFields(raw, base) {
  return Object.keys(tuneLabels).filter((key) => String(raw[key]) !== String(base[key]));
}

function describeValue(key, raw) {
  if (key === "commitRule" || key === "priceMode") {
    return raw[key];
  }
  const percentKeys = ["trust", "objectionRate", "seedFloor", "cap", "returnRate", "priceSignal", "productionVariance"];
  return percentKeys.includes(key) ? `${raw[key]}%` : String(raw[key]);
}

function renderTuneResults(scored, base) {
  tune.results.innerHTML = "";
  scored.forEach((entry, index) => {
    const { raw, metrics: m, sc } = entry;
    const li = document.createElement("li");
    li.className = "tune-card";

    const changes = changedFields(raw, base);
    const changeText = changes.length
      ? changes.map((key) => `${tuneLabels[key]}: ${describeValue(key, raw)}`).join(" · ")
      : "Matches current settings";

    li.innerHTML = `
      <div class="tune-card-head">
        <span class="tune-rank">#${index + 1}</span>
        <span class="tune-score">score ${sc.score.toFixed(2)}</span>
        <button type="button" class="tune-apply">Apply</button>
      </div>
      <p class="tune-explain">${explainCandidate(sc)}</p>
      <dl class="tune-metrics">
        <div><dt>deal trust</dt><dd>${tunePct(m.trust)}</dd></div>
        <div><dt>satisfaction</dt><dd>${tunePct(m.satisfaction)}</dd></div>
        <div><dt>unmet demand</dt><dd>${tunePct(m.unmetFraction)}</dd></div>
        <div><dt>gini</dt><dd>${m.gini.toFixed(2)}</dd></div>
        <div><dt>stewards</dt><dd>${Math.round(m.members)}</dd></div>
        <div><dt>admits</dt><dd>${m.committed}/${m.committed + m.held}</dd></div>
      </dl>
      <p class="tune-changes">${changeText}</p>
    `;
    li.querySelector(".tune-apply").addEventListener("click", () => applyCandidate(raw));
    tune.results.appendChild(li);
  });
}

function applyCandidate(raw) {
  stop();
  for (const [key, ctrl] of Object.entries(controls)) {
    if (key in raw) {
      ctrl.value = raw[key];
    }
  }
  syncOutputs();
  reset();
  tune.status.textContent = "Applied candidate. Sim reset with the new parameters.";
}

function nextTick() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

async function runTune() {
  if (tuning) {
    return;
  }
  tuning = true;
  stop();
  tune.runBtn.disabled = true;
  tune.results.innerHTML = "";

  const budget = tuneBudgets[tune.budget.value] || tuneBudgets.normal;
  const base = rawFromControls();
  const candidates = makeCandidates(budget);
  const scored = [];

  for (let i = 0; i < candidates.length; i += 1) {
    const raw = candidates[i];
    const metrics = evaluateRaw(raw, budget);
    scored.push({ raw, metrics, sc: scoreMetrics(metrics, budget) });
    tune.status.textContent = `Evaluating ${i + 1}/${candidates.length} candidates…`;
    await nextTick();
  }

  scored.sort((a, b) => b.sc.score - a.sc.score);
  const shown = scored.slice(0, 5);
  renderTuneResults(shown, base);
  tune.status.textContent = `Done. Ranked ${scored.length} candidates; showing top ${shown.length}. Run paused.`;
  tune.runBtn.disabled = false;
  tuning = false;
}

tune.runBtn.addEventListener("click", runTune);

for (const input of Object.values(controls)) {
  input.addEventListener("input", () => {
    syncOutputs();
    if (input === controls.population || input === controls.trust) {
      reset();
    } else if (input === controls.runSpeed) {
      if (timer) {
        start();
      }
    } else {
      allocateResources();
      runGoodsEconomy(params(), { quiet: true, recordSignals: false });
      render();
    }
  });
}

ui.stepBtn.addEventListener("click", step);
ui.resetBtn.addEventListener("click", () => {
  stop();
  reset();
});
ui.runBtn.addEventListener("click", () => {
  if (timer) {
    stop();
  } else {
    start();
  }
});
ui.exportBtn.addEventListener("click", exportSummary);
window.addEventListener("resize", render);

function start() {
  stop();
  ui.runBtn.lastElementChild.textContent = "Pause";
  timer = window.setInterval(step, runDelay(params().runSpeed));
}

function stop() {
  if (timer) {
    window.clearInterval(timer);
    timer = null;
  }
  if (ui.runBtn.lastElementChild) {
    ui.runBtn.lastElementChild.textContent = "Run";
  }
}

syncOutputs();
reset();
