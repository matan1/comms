// Comparative Layer-4 acceptance harness. Load after world.js + sim.js.

function vouchRaw(seedOffset, vouchMode) {
  return {
    population: 18, farmShare: 25, trust: 55, gossipRadius: 12,
    gossipDepth: 4, travelWill: 25, sponsors: 2, witnessQuorum: 5,
    objectionRate: 40, arrivalRate: 0, speed: 5, seedOffset, vouchMode
  };
}

function runVouchAttack(name, seedOffset, vouchMode, days = 120, honest = false) {
  const p = normalizeParams(vouchRaw(seedOffset, vouchMode));
  seedState(p);
  for (let i = 0; i < 5; i += 1) advanceDay(p);
  const newcomer = honest ? arrive("honest") : injectAdversary(name);
  for (let i = 0; i < days; i += 1) advanceDay(p);
  const failures = state.attestations.filter((a) =>
    a.type === "deal-record/1" && a.target === newcomer.id
    && a.detail.outcome === "failed" && a.day >= (newcomer.joinedDay ?? Infinity)).length;
  const firstFailure = state.attestations.find((a) =>
    a.type === "deal-record/1" && a.target === newcomer.id && a.detail.outcome === "failed");
  return {
    admitted: newcomer.member,
    joinedDay: newcomer.joinedDay,
    failures,
    detectionLatency: firstFailure && newcomer.joinedDay !== null
      ? firstFailure.day - newcomer.joinedDay : null
  };
}

function mean(xs) {
  return xs.reduce((a, b) => a + b, 0) / Math.max(1, xs.length);
}

function percentile(xs, q) {
  const sorted = [...xs].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))] ?? 0;
}

function runVouchAcceptance(seedCount = 25) {
  const rows = [];
  let flatHarm = 0;
  let vouchHarm = 0;
  let worstRegression = -Infinity;
  for (const name of Object.keys(ADVERSARY_PRESETS)) {
    const flat = [];
    const vouch = [];
    for (let seed = 0; seed < seedCount; seed += 1) {
      flat.push(runVouchAttack(name, seed, false));
      vouch.push(runVouchAttack(name, seed, true));
    }
    const f = flat.reduce((n, x) => n + x.failures, 0);
    const v = vouch.reduce((n, x) => n + x.failures, 0);
    flatHarm += f;
    vouchHarm += v;
    const regression = f ? (v - f) / f : (v ? 1 : 0);
    worstRegression = Math.max(worstRegression, regression);
    rows.push({
      name,
      flatFailures: f,
      vouchFailures: v,
      flatAdmission: mean(flat.map((x) => x.admitted ? 1 : 0)),
      vouchAdmission: mean(vouch.map((x) => x.admitted ? 1 : 0)),
      regression
    });
  }

  const honestFlat = [];
  const honestVouch = [];
  for (let seed = 0; seed < seedCount; seed += 1) {
    honestFlat.push(runVouchAttack("classic", 1000 + seed, false, 120, true));
    honestVouch.push(runVouchAttack("classic", 1000 + seed, true, 120, true));
  }
  const flatHonestRate = mean(honestFlat.map((x) => x.admitted ? 1 : 0));
  const vouchHonestRate = mean(honestVouch.map((x) => x.admitted ? 1 : 0));
  const flatDays = honestFlat.filter((x) => x.joinedDay !== null).map((x) => x.joinedDay);
  const vouchDays = honestVouch.filter((x) => x.joinedDay !== null).map((x) => x.joinedDay);
  const harmReduction = flatHarm ? (flatHarm - vouchHarm) / flatHarm : 0;
  const admissionDrop = flatHonestRate - vouchHonestRate;
  const delay = percentile(vouchDays, .5) - percentile(flatDays, .5);
  const pass = harmReduction >= .25 && admissionDrop <= .05 && delay <= 2
    && worstRegression <= .15;

  console.log(`Vouch acceptance: ${seedCount} seeds x 16 presets x 120 days`);
  for (const r of rows) {
    console.log(`${r.name.padEnd(11)} harm ${String(r.flatFailures).padStart(4)} -> ${String(r.vouchFailures).padStart(4)}`
      + ` | admission ${(r.flatAdmission * 100).toFixed(0)}% -> ${(r.vouchAdmission * 100).toFixed(0)}%`);
  }
  console.log(`aggregate harm reduction: ${(harmReduction * 100).toFixed(1)}%`);
  console.log(`honest admission: ${(flatHonestRate * 100).toFixed(1)}% -> ${(vouchHonestRate * 100).toFixed(1)}%`);
  console.log(`honest median delay: ${delay} days`);
  console.log(`worst preset harm regression: ${(worstRegression * 100).toFixed(1)}%`);
  console.log(pass ? "PASS" : "FAIL");
  return { pass, rows, harmReduction, admissionDrop, delay, worstRegression };
}

const seeds = Number((typeof process !== "undefined" && process.env.VOUCH_SEEDS) || 25);
const result = runVouchAcceptance(seeds);
if (!result.pass && typeof process !== "undefined") process.exitCode = 1;
