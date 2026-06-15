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
if (!members()[0].home.vm) throw new Error("enrolled agent lacks a VM cell");

nextPhase(p);
if (state.phase !== 1 || state.resourceTelemetry.jobs < 1) {
  throw new Error("resource-fabric phase produced no work");
}

const adversary = injectAdversary("sovereign");
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
