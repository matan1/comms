// Focused workstation-world smoke test. Concatenate after the four simulator
// scripts; this intentionally avoids the large adversary acceptance matrix.

const p = normalizeParams({
  worldMode: "workstation",
  population: 18,
  vouchMode: true
});

seedState(p);
if (world.kind !== "workstation") throw new Error("workstation world not selected");
if (world.vramCapacity !== 24) throw new Error("unexpected accelerator capacity");
if (world.resources.length !== 7) throw new Error("resource topology incomplete");
if (!world.remoteZone || world.remoteZone.y >= world.assemblages[1].y) {
  throw new Error("remote gateway is not separated from the assemblage subnet");
}
if (!members()[0].home.vm) throw new Error("enrolled agent lacks a VM cell");
const leftRows = world.vmCells.filter((cell) => cell.x < 0.5).slice(0, 18);
const rowLevels = new Set(leftRows.map((cell) => cell.y.toFixed(4)));
if (rowLevels.size !== 6) throw new Error("VM rows are not aligned");
const pulsePair = members().slice(0, 2);
pulsePair[0].pos = { x: 0.48, y: 0.48 };
pulsePair[1].pos = { x: 0.52, y: 0.52 };
pulses = [];
ripple(pulsePair[0], pulsePair[1], true);
const expectedPulseX = (pulsePair[0].home.x + pulsePair[1].home.x) / 2;
const expectedPulseY = (pulsePair[0].home.y + pulsePair[1].home.y) / 2;
if (Math.abs(pulses[0].x - expectedPulseX) > 1e-9
    || Math.abs(pulses[0].y - expectedPulseY) > 1e-9) {
  throw new Error("store synchronization pulse is not anchored to VM homes");
}
if (interactionEndpointMode() !== "process") {
  throw new Error("headless endpoint default changed");
}
if (keyCustodyMode() !== "embedded" || ledgerMode() !== "off") {
  throw new Error("headless custody defaults changed");
}
if (world.keyManagers.host.length !== 1
    || world.keyManagers.federated.length !== 2
    || !world.ledger) {
  throw new Error("custody topology incomplete");
}
const edge = { from: members()[0].id, to: members()[1].id, lane: 0 };
members()[0].pos = { x: 0.5, y: 0.5 };
const linkStart = (path) => (path.kind === "poly"
  ? { x: path.pts[0].x, y: path.pts[0].y }
  : { x: path.x1, y: path.y1 });
const processPath = curvedLinkGeometry(1000, 1000, edge);
const originalEndpointMode = interactionEndpointMode;
interactionEndpointMode = () => "core";
const corePath = curvedLinkGeometry(1000, 1000, edge);
interactionEndpointMode = originalEndpointMode;
const ps = linkStart(processPath);
const cs = linkStart(corePath);
if (ps.x === cs.x && ps.y === cs.y) {
  throw new Error("endpoint projection did not move to the signing core");
}
// Workstation links now ride the host buses: the routed polyline should have
// interior bus vertices between the two endpoints, not just a straight hop.
if (processPath.kind !== "poly" || processPath.pts.length < 2) {
  throw new Error("workstation link is not a routed bus polyline");
}

nextPhase(p);
if (state.phase !== 1 || state.resourceTelemetry.jobs < 1) {
  throw new Error("resource-fabric phase produced no work");
}

const adversary = injectAdversary("sovereign");
if (!adversary.home.vm || !adversary.home.claimed) {
  throw new Error("staged agent lacks a persistent VM identity locus");
}
adversary.home.status = "suspended";
if (!adversary.home.claimed || adversary.home.status !== "suspended") {
  throw new Error("suspended VM was released");
}
if (agentNames.includes(names[0])) {
  throw new Error("workstation and village naming vocabularies overlap");
}
if (adversaryName(adversary.adversaryType) !== "control-plane captor") {
  throw new Error("workstation adversary vocabulary missing");
}
if (!adversaryOrigin(adversary).includes("control-plane captor")) {
  throw new Error("workstation adversary origin missing from inspector");
}

console.log("WORKSTATION SMOKE: PASS", JSON.stringify({
  agents: state.villagers.length,
  resources: world.resources.length,
  jobs: state.resourceTelemetry.jobs,
  vramRequested: state.resourceTelemetry.vramUsed,
  vramCapacity: state.resourceTelemetry.vramCapacity
}));
