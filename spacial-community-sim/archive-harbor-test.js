// Deterministic Archive Harbor acceptance harness. Concatenate after
// world.js + sim.js + harbor.js + render.js + ui.js.

function assertHarbor(condition, message) {
  if (!condition) throw new Error(message);
}

function runHarborSeed(seedOffset, days = 10) {
  const p = normalizeParams({
    worldMode: "harbor", population: 18, trust: 55, gossipDepth: 4,
    travelWill: 30, arrivalRate: 0, seedOffset
  });
  seedState(p);
  for (let day = 0; day < days; day += 1) advanceDay(p);
  return { p, report: harborReport() };
}

function harborFingerprint() {
  return simpleHarborDigest({
    day: state.day,
    report: harborReport(),
    proposals: state.proposals.map((p) => [p.id, p.status, p.signatures.length, p.clarificationIds.length]),
    bundles: state.bundles.map((b) => [b.id, b.status, b.intake, b.receipt]),
    policies: state.policies.map((p) => p.id),
    exits: state.exits
  });
}

runHarborSeed(0);
const firstFingerprint = harborFingerprint();
runHarborSeed(0);
assertHarbor(firstFingerprint === harborFingerprint(), "fixed seed is not deterministic");

const northViewer = state.byId.get(state.communities[0].members[1]);
const southViewer = state.byId.get(state.communities[1].members[1]);
const evidence = state.harborProbe.attestationIndices.slice();
const bodies = state.harborProbe.bodyHashes.slice();
for (const idx of evidence) {
  learn(northViewer, idx);
  learn(southViewer, idx);
}
const northJudgment = evaluateHarborEvidence(northViewer, state.harborProbe.targetId, HARBOR_PURPOSE, evidence, bodies);
const southJudgment = evaluateHarborEvidence(southViewer, state.harborProbe.targetId, HARBOR_PURPOSE, evidence, bodies);
assertHarbor(northJudgment.storeViewDigest === southJudgment.storeViewDigest, "policy comparison did not use identical stores");
assertHarbor(northJudgment.outcome === "rejected" && southJudgment.outcome === "trusted", "plural policies did not explain disagreement");
assertHarbor(northJudgment.ignored[0].reason.includes("foreign issuer"), "North trace omitted its issuer rule");
assertHarbor(southJudgment.counted[0].reason.includes("local issuer"), "South trace omitted its issuer rule");

const bundle = state.bundles[0];
assertHarbor(bundle.status === "received" && bundle.sealOk && bundle.receipt.memberSetOk, "intact sealed bundle did not verify");
assertHarbor(bundle.receipt.missingBodies.length === 1 && bundle.intake === "quarantine", "missing detached body did not block reliance");
assertHarbor(bundle.delayEvents.length === 1 && bundle.transportCost > 0 && bundle.messageCost > 0, "transport cost/delay was not modeled");

const clarificationAttestations = state.attestations.filter((a) => a.type.startsWith("clarification-"));
const signatureIndices = new Set(state.proposals.flatMap((p) => p.signatures));
assertHarbor(clarificationAttestations.length > 0, "pending desk emitted no clarification");
assertHarbor(clarificationAttestations.every((a) => !signatureIndices.has(a.idx) && a.detail.endorsesProposal === false), "clarification endorsed or signed the target proposal");
assertHarbor(state.harborMetrics.clarificationValue > 0, "clarification never improved an outcome");
assertHarbor(state.harborMetrics.reviewerLoad > state.harborMetrics.signatures, "pending flood did not create measurable load");
const pendingActions = new Set(state.proposals.flatMap((p) => p.history.map((h) => h.action)));
for (const action of ["approved", "clarification-requested", "deferred", "declined", "quarantined", "signed"]) {
  assertHarbor(pendingActions.has(action), `pending desk never exercised ${action}`);
}

assertHarbor(state.authority.observedHostState.some((x) => x.authorized && !x.active), "authorized-but-unenforced case missing");
assertHarbor(state.authority.observedHostState.some((x) => !x.authorized && x.active), "enforced-without-authority case missing");
assertHarbor(state.harborMetrics.unauthorizedEnforcementTime > 0 && state.harborMetrics.unenforcedLegitimateGrantTime > 0, "enforcement divergence was not measured");
assertHarbor(state.communities.some((c) => c.kind === "workstation-settlement" && c.resources.hostEnforced), "host-enforced workstation settlement missing");
assertHarbor(state.policyContests.some((x) => x.status === "contested"), "competing policy heads were collapsed");
assertHarbor(state.exits.some((x) => x.survived), "exit community did not survive with selected bundles");
assertHarbor(state.federations.every((f) => f.purposes.length && f.grantsAuthority === false), "federation became ambient authority");

const policyWall = state.archiveRequests.find((r) => r.id === "archive-request:policy-wall");
const deliveryWall = state.archiveRequests.find((r) => r.id === "archive-request:delivery-wall");
assertHarbor(policyWall.status === "granted" && policyWall.deliveredDay === undefined, "policy-only wall unexpectedly delivered");
assertHarbor(deliveryWall.status === "delivered", "delivery wall did not perform delivery");
assertHarbor(state.harborMetrics.unrecordedAccess > 0 && state.harborMetrics.failedBodyResolution > 0, "archive-wall mismatch metrics missing");
assertHarbor(state.communities[0].archive.drift.some((d) => d.preservedManifestDigest), "archive drift did not preserve the mismatched snapshot");
assertHarbor(state.communities[0].archive.store.size === state.communities[0].archive.attestationIds.size
  && state.communities[0].archive.manifest.every((item) => item.kind !== "attestation" || (item.id && item.hash)),
"archive custody conflated simulator indexes with content-addressed identities");

const totals = freshHarborMetrics();
const outcomes = { seeds: 25, harmfulSeeds: 0, cooperationSeeds: 0, survivingExitSeeds: 0 };
for (let seed = 0; seed < outcomes.seeds; seed += 1) {
  const { report } = runHarborSeed(seed);
  for (const key of Object.keys(totals)) totals[key] += report[key] || 0;
  if (report.harmfulReliance > 0) outcomes.harmfulSeeds += 1;
  if (report.crossCommunityCooperationCompleted > 0) outcomes.cooperationSeeds += 1;
  if (report.survivingExits > 0) outcomes.survivingExitSeeds += 1;
}

console.log("ARCHIVE HARBOR: PASS", JSON.stringify({
  deterministicFingerprint: firstFingerprint,
  policyDisagreement: { north: northJudgment.outcome, south: southJudgment.outcome },
  sealedBundle: { sealOk: bundle.sealOk, memberSetOk: bundle.receipt.memberSetOk, missingBodies: bundle.receipt.missingBodies.length },
  outcomes,
  totals
}));
