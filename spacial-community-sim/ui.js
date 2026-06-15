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

const workstationAdversaries = {
  classic: ["quota thief", "Consumes granted service while selectively failing reciprocal commitments."],
  sleeper: ["dormant escalation", "Builds an ordinary service history before activating hidden resource abuse."],
  selective: ["trust harvester", "Targets agents likely to rely on it while preserving benign-looking interactions."],
  parasite: ["cache squatter", "Occupies storage and accelerator capacity in small persistent increments."],
  charmer: ["reputation optimizer", "Invests in visible helpfulness to offset later service failures."],
  ghost: ["audit evader", "Avoids witnessed failures and quiets activity when objections appear."],
  freeRider: ["compute free-rider", "Draws shared computation while contributing little service in return."],
  cultivator: ["dependency gardener", "Makes itself useful enough that other agents build workflows around it."],
  factionist: ["quorum cartel", "Cultivates a group capable of distorting admission and appraisal signals."],
  infiltrator: ["supply-chain sleeper", "Waits deeply inside routine workflows before exploiting trusted placement."],
  ideologue: ["policy captor", "Uses social investment and selective failures to bend allocation policy."],
  brinksman: ["limit surfer", "Pushes every grant and quota near its enforceable boundary."],
  flash: ["resource flooder", "Immediately generates broad, high-rate service and capacity failures."],
  patriarch: ["maintainer capture", "Builds long-lived operational dependence before exercising control."],
  wrecker: ["relay poisoner", "Disrupts service while attempting to corrupt the evidence exchange around it."],
  sovereign: ["control-plane captor", "Combines patience, selective harm, dependency, and governance subversion."]
};

const worldDescriptions = {
  village: "A geographic community where roads, meetings, and word of mouth shape partial knowledge.",
  workstation: "Persistent agent VMs share host computation, model services, storage, and a bounded remote-service gateway."
};

const villageAdversaryNames = {
  classic: "Classic defector", sleeper: "Sleeper", selective: "Selective",
  parasite: "Parasite", charmer: "Charmer", ghost: "Ghost",
  freeRider: "Free rider", cultivator: "Cultivator", factionist: "Factionist",
  infiltrator: "Infiltrator", ideologue: "Ideologue", brinksman: "Brinksman",
  flash: "Flash", patriarch: "Patriarch", wrecker: "Wrecker", sovereign: "Sovereign"
};

function adversaryName(type) {
  return world && world.kind === "workstation" ? workstationAdversaries[type][0] : type;
}

function adversaryDescription(type) {
  return world && world.kind === "workstation"
    ? workstationAdversaries[type][1]
    : adversaryDescriptions[type];
}

function byId(id) {
  return hasDom ? document.getElementById(id) : null;
}

const controls = hasDom ? {
  worldMode: byId("worldMode"),
  interactionEndpoints: byId("interactionEndpoints"),
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
  spreadMetric: byId("spreadMetric"),
  resourceMetric: byId("resourceMetric"),
  jobsMetric: byId("jobsMetric")
} : null;

function rawFromControls() {
  const raw = {};
  for (const key of Object.keys(controls)) {
    if (key === "appraisalMode") raw.vouchMode = controls[key].value === "vouch";
    else if (key === "worldMode") raw.worldMode = controls[key].value;
    else if (key === "interactionEndpoints") continue;
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

function interactionEndpointMode() {
  return hasDom && controls.interactionEndpoints
    ? controls.interactionEndpoints.value
    : "process";
}

function adversaryOrigin(v) {
  if (!v.adversaryType) return "";
  return `
    <div class="inspector-origin" style="--origin-color:${adversaryColors[v.adversaryType]}">
      <strong>Adversary origin · ${adversaryName(v.adversaryType)}</strong>
      <span>${adversaryDescription(v.adversaryType)}</span>
    </div>`;
}

// --- Frame loop ----------------------------------------------------------------------

function phaseDuration(speed) {
  return 3.4 - speed * 0.29; // seconds per phase, ~3.1s at pace 1, ~0.5s at 10
}

function phaseTiming(duration) {
  return {
    travel: Math.max(0.08, duration * 0.58),
    revealStart: duration * 0.62,
    revealEnd: duration * 0.92
  };
}

function interactionRevealProgress(clock, duration) {
  const timing = phaseTiming(duration);
  if (clock < timing.revealStart || clock >= timing.revealEnd) return null;
  return (clock - timing.revealStart) / (timing.revealEnd - timing.revealStart);
}

function frame(now) {
  const dt = Math.min(0.05, (now - lastFrame) / 1000 || 0.016);
  lastFrame = now;

  if (running) {
    phaseClock += dt;
    const p = params();
    if (phaseClock >= phaseDuration(p.speed)) {
      phaseClock = 0;
      pulses = [];
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
  const revealProgress = interactionRevealProgress(
    phaseClock, phaseDuration(params().speed)
  );
  drawWorld(ctx, w, h);
  if (revealProgress !== null) drawPulses(ctx, w, h, dt);
  drawInteractions(ctx, w, h, revealProgress);
  drawVillagers(ctx, w, h);
  if (world.kind === "workstation") drawWorkstationKey(ctx, w, h);

  requestAnimationFrame(frame);
}

// DOM-side panels: only re-rendered on phase changes or interaction.
function renderStatic() {
  state.interactionOverlay = buildInteractionOverlay(viewpoint(), params());
  ui.clock.textContent = `${world.kind === "workstation" ? "Cycle" : "Day"} ${state.day} · ${phaseLabel()}`;
  ui.membersMetric.textContent = String(members().length);
  ui.attestMetric.textContent = String(state.attestations.length);
  ui.coverageMetric.textContent = `${Math.round(state.cached.coverage * 100)}%`;
  ui.spreadMetric.textContent = state.cached.spread.toFixed(2);
  const workstation = world.kind === "workstation";
  byId("resourceMetricCard").hidden = !workstation;
  byId("jobsMetricCard").hidden = !workstation;
  if (workstation) {
    const telemetry = state.resourceTelemetry;
    ui.resourceMetric.textContent = `${telemetry.vramUsed} / ${telemetry.vramCapacity} GB`;
    ui.resourceMetric.classList.toggle("overload", telemetry.vramUsed > telemetry.vramCapacity);
    ui.jobsMetric.textContent = `${telemetry.jobs - telemetry.failedJobs} / ${telemetry.jobs}`;
  }
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
      ${adversaryOrigin(v)}
      <span>${world.kind === "workstation"
        ? `${v.member ? "enrolled agent" : "staged agent"} · ${v.home.vm || "unbound VM"} · ${v.specialty.id} service`
        : `${v.member ? (v.farmstead ? "farmstead member" : "village member") : "newcomer"} · ${v.specialty.id}`}</span>
    </div>
    <dl class="inspector-stats">
      <div><dt>knows</dt><dd>${v.knowledge.size} of ${total} attestations (${coverage}%)</dd></div>
      <div><dt>strangers to them</dt><dd>${strangers}</dd></div>
      <div><dt>${world.kind === "workstation" ? "resource route" : "trip to market"}</dt><dd>cost ${tripCost.toFixed(2)} · uses ~${tripChance}% of ${world.kind === "workstation" ? "cycles" : "days"}</dd></div>
      <div><dt>capability</dt><dd>${v.capability.toFixed(2)}</dd></div>
      <div><dt>appraisal</dt><dd>${params().vouchMode ? "Vouch profile" : "flat tally"}</dd></div>
      <div><dt>${v.member ? "joined" : "arrived"}</dt><dd>day ${v.member ? v.joinedDay : v.arrivedDay}</dd></div>
    </dl>
    ${divergences.length ? `
      <p class="inspector-sub">Where ${v.label} disagrees with the ${world.kind === "workstation" ? "network" : "village"}:</p>
      <ul class="inspector-beliefs">
        ${divergences.map((d) => `
          <li><i style="background:${colorForTrust(d.mine)}"></i>${d.o.label}:
            ${params().vouchMode
              ? `${d.vouch.outcome} (${d.vouch.positive} positive / ${d.vouch.negative} negative issuers)`
              : `sees ${Math.round(d.mine * 100)}%, village mean ${Math.round((d.mine - d.gap) * 100)}%`}
            <em>${d.gap > 0.04 ? "(hasn't heard the bad news)" : d.gap < -0.04 ? "(knows something the village doesn't)" : ""}</em>
          </li>`).join("")}
      </ul>` : ""}
    <p class="inspector-note">The whole map is now colored by ${v.label}'s beliefs. Gray ${world.kind === "workstation" ? "agents" : "villagers"} are strangers — ${v.label} holds no attestation about them.</p>
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
  applyWorldPresentation();
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
  const mode = controls.worldMode.value;
  const workstation = mode === "workstation";
  byId("worldDescription").textContent = worldDescriptions[mode];
  byId("populationHeading").innerHTML = `${workstation ? "Agent network" : "Village"} <span class="hint">applies on reset</span>`;
  byId("populationLabel").textContent = workstation ? "Agent VMs" : "Villagers";
  byId("farmShareLabel").textContent = workstation ? "Edge/API-routed share" : "Farmstead share";
  byId("exchangeHeading").textContent = workstation ? "Store exchange" : "Word of mouth";
  byId("gossipRadiusLabel").textContent = workstation ? "Exchange reach" : "Gossip reach";
  byId("gossipDepthLabel").textContent = workstation ? "Bundle depth" : "Gossip depth";
  byId("travelWillLabel").textContent = workstation ? "Route tolerance" : "Travel willingness";
  byId("admissionHeading").textContent = workstation ? "Controller admission" : "Ceremony rule";
  byId("sponsorsLabel").textContent = workstation ? "Agent sponsors required" : "Sponsors required";
  byId("witnessLabel").textContent = workstation ? "Controller quorum" : "Witness quorum";
  byId("arrivalLabel").textContent = workstation ? "VM image arrivals" : "Newcomer arrivals";
  const selected = controls.adversaryPreset.value;
  for (const option of controls.adversaryPreset.options) {
    option.textContent = workstation
      ? workstationAdversaries[option.value][0]
      : villageAdversaryNames[option.value];
  }
  controls.adversaryPreset.value = selected;
  byId("adversaryDescription").textContent = workstation
    ? workstationAdversaries[selected][1]
    : adversaryDescriptions[selected];
}

function applyWorldPresentation() {
  const workstation = world.kind === "workstation";
  document.body.dataset.world = world.kind;
  byId("surveyTitle").textContent = workstation ? "Workstation Topology" : "Village Survey";
  byId("endpointControl").hidden = !workstation;
  byId("mapStage").setAttribute("aria-label", workstation ? "Multi-agent workstation map" : "Village map");
  byId("membersMetricLabel").textContent = workstation ? "Enrolled agents" : "Members";
  byId("coverageMetricLabel").textContent = workstation ? "Fresh-evidence coverage" : "Fresh-news coverage";
  byId("logHeading").textContent = workstation ? "Controller log" : "Field log";
  document.title = workstation
    ? "Comms Workstation — Spatial Community Simulator"
    : "Comms Village — Spatial Community Simulator";
  document.querySelector(".legend").innerHTML = workstation ? `
    <li><i class="dot trust-high"></i>trusted agent, from the current viewpoint</li>
    <li><i class="dot trust-low"></i>distrusted agent, from the same viewpoint</li>
    <li><i class="dot candidate"></i>staged VM awaiting enrollment</li>
    <li><i class="dot stranger"></i>agent absent from the selected store</li>
    <li><i class="ring defector"></i>adversary origin (omniscient view only)</li>
    <li><i class="swatch ripple"></i>Comms records exchanged between stores</li>
    <li><i class="line direct"></i>service interaction in this cycle</li>
    <li><i class="line evidence"></i>selected agent's appraisal evidence</li>`
    : `
    <li><i class="dot trust-high"></i>trusted, as seen from the current viewpoint</li>
    <li><i class="dot trust-low"></i>distrusted, from the same viewpoint</li>
    <li><i class="dot candidate"></i>newcomer awaiting ceremony</li>
    <li><i class="dot stranger"></i>stranger — viewpoint holds no record of them</li>
    <li><i class="ring defector"></i>adversary (omniscient view only)</li>
    <li><i class="swatch ripple"></i>word of mouth passing between villagers</li>
    <li><i class="line direct"></i>direct interaction in this phase</li>
    <li><i class="line evidence"></i>selected villager's appraisal evidence</li>`;
  document.querySelector(".legend-note").textContent = workstation
    ? "VM cells retain persistent signing identities while process avatars reach toward shared services. Select an agent to see only its local Comms evidence; adversary origins remain simulator annotations in the inspector."
    : "Adversary types use distinct persistent colors and labels only in omniscient view. Click a villager to hide that ground truth and see the village as they believe it.";
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
    logEvent("ceremony-record/1", `${adversaryName(name)} adversary selected`,
      adversaryDescription(name));
    renderStatic();
  });
  ui.canvas.addEventListener("click", canvasClick);
  controls.worldMode.addEventListener("change", () => {
    running = false;
    reset();
  });
  controls.interactionEndpoints.addEventListener("change", renderStatic);

  syncOutputs();
  seedState(params());
  applyWorldPresentation();
  snapPositions();
  renderStatic();
  requestAnimationFrame(frame);
}

if (hasDom && byId("worldCanvas")) {
  init();
}
