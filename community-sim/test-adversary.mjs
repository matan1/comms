// Headless adversary test. Run with: node test-adversary.mjs
// Verifies that each adversary behavior (betrayal, endorsement farming,
// lie-low, activation delay) fires within the expected range over 80 days.

import { readFileSync } from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";
import vm from "vm";

const __dir = dirname(fileURLToPath(import.meta.url));

// Patch the minimal globals app.js needs when run headless.
global.document = { getElementById: () => null };
global.window = {};
global.performance = { now: () => 0 };
global.requestAnimationFrame = () => {};

// runInThisContext injects module-level declarations into the current scope.
const src = readFileSync(join(__dir, "app.js"), "utf8");
vm.runInThisContext(src);

function runScenario(label, apOverrides, days = 80) {
  const ap = {
    activationDelay: 0, defectRate: 0.55, selectivity: 0, coverRate: 0,
    socialInvestment: 0, recoveryRate: 0, networkSubversion: 0,
    ...apOverrides
  };

  // Build raw params that normalizeParams expects (slider units).
  const raw = {
    population: 18, farmShare: 25, trust: 55, gossipRadius: 12,
    gossipDepth: 4, marketAttend: 25, sponsors: 2, witnessQuorum: 5,
    objectionRate: 40, arrivalRate: 0, speed: 5,
    aDelay: ap.activationDelay,
    aDefect: Math.round(ap.defectRate * 100),
    aSelect: Math.round(ap.selectivity * 100),
    aCover: Math.round(ap.coverRate * 100),
    aSocial: Math.round(ap.socialInvestment * 100),
    aRecovery: Math.round(ap.recoveryRate * 100),
    aFaction: Math.round(ap.networkSubversion * 100),
  };
  const p = normalizeParams(raw);

  seedState(p);

  // Let the village settle for 5 days, then inject.
  for (let i = 0; i < 5; i++) advanceDay(p);
  const adv = arrive("adversary", p.adversary);

  for (let i = 0; i < days; i++) advanceDay(p);

  // Count events involving the adversary.
  const advAtts = state.attestations.filter(a => a.target === adv.id || a.by === adv.id);
  const betrayals = advAtts.filter(a =>
    a.type === "deal-record/1" && a.by !== adv.id && a.detail.outcome === "failed").length;
  const endorsementsFarmed = advAtts.filter(a =>
    a.type === "endorsement/1" && a.target === adv.id && a.by !== "comms.steward:zVILLAGE").length;

  console.log(`\n[${label}]`);
  console.log(`  admitted=${adv.member}  betrayals=${betrayals}`);
  console.log(`  endorsements farmed=${endorsementsFarmed}  lyingLowUntil=${adv.lyingLowUntil}`);
  return { admitted: adv.member, betrayals, endorsementsFarmed };
}

let pass = 0;
let fail = 0;
function assert(cond, msg) {
  if (cond) { console.log(`  PASS: ${msg}`); pass++; }
  else       { console.error(`  FAIL: ${msg}`); fail++; }
}

// Classic: high betrayal rate.
const classic = runScenario("Classic", { defectRate: 0.55 });
assert(classic.betrayals > 0, "classic: at least one betrayal");

// Sleeper: no betrayals before day 30 of membership.
{
  const p = normalizeParams({
    population: 18, farmShare: 25, trust: 55, gossipRadius: 12,
    gossipDepth: 4, marketAttend: 25, sponsors: 2, witnessQuorum: 5,
    objectionRate: 40, arrivalRate: 0, speed: 5,
    aDelay: 30, aDefect: 70, aSelect: 0, aCover: 0, aSocial: 0, aRecovery: 0, aFaction: 0,
  });
  seedState(p);
  const adv = arrive("adversary", p.adversary);
  // Admit manually for a clean test.
  adv.member = true;
  adv.joinedDay = state.day;
  for (let i = 0; i < 25; i++) advanceDay(p);
  const earlyBetrays = state.attestations.filter(a =>
    a.target === adv.id && a.type === "deal-record/1" && a.detail.outcome === "failed").length;
  assert(earlyBetrays === 0, `sleeper: zero betrayals in first 25 days (got ${earlyBetrays})`);
  for (let i = 0; i < 20; i++) advanceDay(p);
  const lateBetrays = state.attestations.filter(a =>
    a.target === adv.id && a.type === "deal-record/1" && a.detail.outcome === "failed").length;
  assert(lateBetrays > 0, `sleeper: betrayals appear after delay (got ${lateBetrays})`);
}

// Charmer: endorsement farming fires measurably.
const charmer = runScenario("Charmer", { defectRate: 0.40, socialInvestment: 0.90 });
assert(charmer.endorsementsFarmed > 0, `charmer: endorsements farmed (got ${charmer.endorsementsFarmed})`);

// Ghost: high cover reduces witnessed betrayals; lie-low kicks in when caught.
// Cover suppresses betrayals when witnesses > 0, so fewer than classic but not zero.
const ghost = runScenario("Ghost", { defectRate: 0.90, coverRate: 1.0, recoveryRate: 1.0 });
assert(ghost.betrayals < classic.betrayals,
  `ghost: fewer betrayals than classic due to cover (${ghost.betrayals} < ${classic.betrayals})`);

// Free Rider: lower defect rate → fewer betrayals than classic.
const freeRider = runScenario("Free Rider", { defectRate: 0.15 });
assert(freeRider.betrayals < classic.betrayals,
  `free rider: fewer betrayals than classic (${freeRider.betrayals} < ${classic.betrayals})`);

console.log(`\n${pass} passed, ${fail} failed`);
if (fail > 0) process.exit(1);
