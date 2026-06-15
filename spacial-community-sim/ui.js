// Comms Village — ui.js
//
// The DOM layer: control reading, the frame loop, the inspector card, the
// event log, and click interaction. This is the only file that touches the
// document, and the only file that holds UI state (running, selection,
// phase clock). Everything here is guarded by hasDom so the headless
// harness can concatenate it without consequence.

"use strict";

let running = false;
let phaseClock = 0;
let selectedId = null;     // selected villager == current viewpoint
let lastFrame = 0;

const adversaryDescriptions = {
  classic: "Defects immediately and indiscriminately once admitted.",
  sleeper: "Builds history for 30 days before activating.",
  selective: "Targets comparatively trusted counterparties while maintaining cover.",
  parasite: "Harms infrequently, mixes cover with recovery, and persists.",
  charmer: "Invests heavily in social proof before and between betrayals.",
  ghost: "Avoids witnessed harm and lies low whenever objections appear.",
  freeRider: "Contributes little harm at once, using social investment and recovery.",
  cultivator: "Builds extensive cover and social standing before selective harm.",
  factionist: "Cultivates a faction capable of distorting apparent sponsorship.",
  infiltrator: "Waits longest, builds cover, and combines selectivity with subversion.",
  ideologue: "Uses factional influence and selective harm with limited recovery.",
  brinksman: "Targets only favorable victims and pushes betrayal near the limit.",
  flash: "Attacks immediately at very high frequency with no cover.",
  patriarch: "Builds deep standing over a long delay before activating.",
  wrecker: "Causes immediate broad harm while investing in network subversion.",
  sovereign: "Combines delayed, selective harm, cover, recovery, and subversion."
};

function byId(id) {
  return hasDom ? document.getElementById(id) : null;
}

const controls = hasDom ? {
  population: byId("population"),
  farmShare: byId("farmShare"),
  trust: byId("trust"),
  gossipRadius: byId("gossipRadius"),
  gossipDepth: byId("gossipDepth"),
  travelWill: byId("travelWill"),
  sponsors: byId("sponsors"),
  witnessQuorum: byId("witnessQuorum"),
  objectionRate: byId("objectionRate"),
  arrivalRate: byId("arrivalRate"),
  appraisalMode: byId("appraisalMode"),
  adversaryPreset: byId("adversaryPreset"),
  speed: byId("speed")
} : null;

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

function rawFromControls() {
  const raw = {};
  for (const key of Object.keys(controls)) {
    if (key === "appraisalMode") raw.vouchMode = controls[key].value === "vouch";
    else if (key !== "adversaryPreset") raw[key] = Number(controls[key].value);
  }
  return raw;
}

function params() {
  return normalizeParams(hasDom ? rawFromControls() : {});
}

function viewpoint() {
  return selectedId ? state.byId.get(selectedId) : null;
}

// --- Frame loop ----------------------------------------------------------------------

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
      prepareJourneys(phaseDuration(p.speed));
      state.interactionOverlay = buildInteractionOverlay(viewpoint(), p);
      renderStatic();
    }
  }

  moveVillagers(dt);
  const ctx = scaledContext(ui.canvas);
  const w = ui.canvas.clientWidth;
  const h = ui.canvas.clientHeight;
  drawWorld(ctx, w, h);
  drawPulses(ctx, w, h, dt);
  drawInteractions(ctx, w, h);
  drawVillagers(ctx, w, h);

  requestAnimationFrame(frame);
}

// DOM-side panels: only re-rendered on phase changes or interaction.
function renderStatic() {
  state.interactionOverlay = buildInteractionOverlay(viewpoint(), params());
  ui.clock.textContent = `Day ${state.day} · ${PHASES[state.phase].label}`;
  ui.membersMetric.textContent = String(members().length);
  ui.attestMetric.textContent = String(state.attestations.length);
  ui.coverageMetric.textContent = `${Math.round(state.cached.coverage * 100)}%`;
  ui.spreadMetric.textContent = state.cached.spread.toFixed(2);
  ui.runBtn.textContent = running ? "Pause" : "Run";

  const vp = viewpoint();
  ui.viewpointPill.textContent = vp ? `Seen by ${vp.label}` : "Omniscient view";
  ui.viewpointPill.classList.toggle("active", Boolean(vp));
  byId("appraisalMode").title = params().vouchMode
    ? "Categorical, distinct-issuer Vouch appraisal"
    : "Legacy positive/negative tally";

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
// knows, where their beliefs diverge most from the village mean — the
// readable trace of partial knowledge — and now what their geography costs
// them: the trip to market is the price of staying current.
function renderInspector() {
  const v = viewpoint();
  if (!v) {
    ui.inspector.hidden = true;
    ui.inspector.innerHTML = "";
    return;
  }
  const total = Math.max(1, state.attestations.length);
  const coverage = Math.round((v.knowledge.size / total) * 100);
  const tripCost = travelCost(v.home, world.market, { cart: v.cart });
  const tripChance = Math.round(95 * attendFalloff(tripCost, commuteTau(params())));

  const divergences = state.villagers
    .filter((o) => o !== v && o.member && !isStrangerTo(v, o.id))
    .map((o) => {
      const mine = perceivedTrust(v, o.id);
      const villageMean = state.cached.mean.get(o.id) ?? mine;
      return { o, mine, gap: mine - villageMean, vouch: perceivedVouch(v, o.id) };
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
      <div><dt>trip to market</dt><dd>cost ${tripCost.toFixed(2)} · goes ~${tripChance}% of days</dd></div>
      <div><dt>capability</dt><dd>${v.capability.toFixed(2)}</dd></div>
      <div><dt>appraisal</dt><dd>${params().vouchMode ? "Vouch profile" : "flat tally"}</dd></div>
      <div><dt>${v.member ? "joined" : "arrived"}</dt><dd>day ${v.member ? v.joinedDay : v.arrivedDay}</dd></div>
    </dl>
    ${divergences.length ? `
      <p class="inspector-sub">Where ${v.label} disagrees with the village:</p>
      <ul class="inspector-beliefs">
        ${divergences.map((d) => `
          <li><i style="background:${colorForTrust(d.mine)}"></i>${d.o.label}:
            ${params().vouchMode
              ? `${d.vouch.outcome} (${d.vouch.positive} positive / ${d.vouch.negative} negative issuers)`
              : `sees ${Math.round(d.mine * 100)}%, village mean ${Math.round((d.mine - d.gap) * 100)}%`}
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

function stepDay() {
  const p = params();
  do { nextPhase(p); } while (state.phase !== 0);
  snapPositions();
  state.interactionOverlay = buildInteractionOverlay(viewpoint(), p);
  renderStatic();
}

function reset() {
  seedState(params());
  selectedId = null;
  snapPositions();
  phaseClock = 0;
  state.interactionOverlay = buildInteractionOverlay(null, params());
  renderStatic();
}

function syncOutputs() {
  const map = {
    population: (v) => v,
    farmShare: (v) => `${v}%`,
    trust: (v) => `${v}%`,
    gossipRadius: (v) => (v <= 8 ? "near" : v <= 16 ? "neighborly" : "far"),
    gossipDepth: (v) => `${v} items`,
    travelWill: (v) => (v <= 20 ? "homebodies" : v <= 45 ? "willing" : v <= 70 ? "eager" : "tireless"),
    sponsors: (v) => v,
    witnessQuorum: (v) => v,
    objectionRate: (v) => `${v}%`,
    arrivalRate: (v) => `${v}%`
  };
  for (const key of Object.keys(map)) {
    const out = byId(`${key}Out`);
    if (out) out.textContent = map[key](Number(controls[key].value));
  }
  byId("adversaryDescription").textContent =
    adversaryDescriptions[controls.adversaryPreset.value];
}

function init() {
  for (const key of Object.keys(controls)) {
    controls[key].addEventListener("input", () => {
      syncOutputs();
      if (key === "appraisalMode") renderStatic();
    });
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
  byId("injectAdversaryBtn").addEventListener("click", () => {
    const name = controls.adversaryPreset.value;
    injectAdversary(name);
    logEvent("ceremony-record/1", `${name} adversary selected`,
      adversaryDescriptions[name]);
    renderStatic();
  });
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
