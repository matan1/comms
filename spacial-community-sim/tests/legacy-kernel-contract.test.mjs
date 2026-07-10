import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const files = ["world.js", "sim.js", "harbor.js", "render.js", "ui.js", "legacy-bridge.js"];
const source = files.map((file) => readFileSync(new URL(`../${file}`, import.meta.url), "utf8")).join("\n");

function scenario(seedOffset = 0) {
  const context = vm.createContext({ console, structuredClone });
  vm.runInContext(`${source}\nthis.__drive = () => {
    const p = normalizeParams({ worldMode: "harbor", population: 18, arrivalRate: 0, seedOffset: ${seedOffset} });
    seedState(p);
    for (let day = 0; day < 10; day += 1) advanceDay(p);
    return {
      snapshot: CommsSimulation.getSnapshot(),
      report: harborReport(),
      fingerprint: simpleHarborDigest({ report: harborReport(), proposals: state.proposals.map((proposal) => [proposal.id, proposal.status]), bundles: state.bundles.map((bundle) => bundle.receipt) })
    };
  };`, context);
  return context.__drive();
}

test("legacy Harbor behavior remains deterministic behind the kernel facade", () => {
  const first = scenario(0);
  const second = scenario(0);
  assert.equal(first.fingerprint, second.fingerprint);
  assert.equal(first.report.bundleDeliveries, 1);
  assert.equal(first.report.failedBodyResolution, 1);
  assert.equal(first.report.unauthorizedEnforcementTime, first.report.unenforcedLegitimateGrantTime);
  assert.equal(first.report.survivingExits, 1);
});

test("bridge exports a plain participant-safe projection source", () => {
  const { snapshot } = scenario(4);
  assert.equal(snapshot.schemaVersion, 1);
  assert.equal(snapshot.worldKind, "harbor");
  assert.equal(snapshot.settlements.length, 3);
  assert.equal(snapshot.couriers[0].status, "delivered");
  assert.ok(snapshot.attestations.length > 0);
  assert.equal(JSON.stringify(snapshot).includes("research"), false);
  assert.equal(JSON.stringify(snapshot).includes("harmful"), false);
  assert.doesNotThrow(() => structuredClone(snapshot));
});

