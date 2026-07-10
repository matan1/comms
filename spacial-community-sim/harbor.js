// Archive Harbor — plural policy, custody, transport, pending attention,
// authority/enforcement, and exit. This layer deliberately reuses sim.js's
// viewer-relative stores and attestations instead of introducing a second
// trust engine.

"use strict";

const HARBOR_PURPOSE = "cross-harbor-trade";

function harborCommunity(id) {
  return state.communities.find((c) => c.id === id) || null;
}

function harborPolicy(id) {
  return state.policies.find((policy) => policy.id === id) || null;
}

function initializeArchiveHarbor(p) {
  const [northLayout, southLayout] = world.settlements;
  const northMembers = state.villagers.filter((v) => v.communityId === northLayout.id);
  const southMembers = state.villagers.filter((v) => v.communityId === southLayout.id);
  state.policies = [
    {
      id: "comms.policy:north-conservative-v1", community: northLayout.id,
      predecessor: null, purposes: [HARBOR_PURPOSE], interpreterVersion: "harbor-1",
      thresholds: { positiveIssuers: 2 }, bodyRules: { detached: "required" },
      issuerRules: { mode: "local-only", namedNeighbors: [] }
    },
    {
      id: "comms.policy:south-trading-v1", community: southLayout.id,
      predecessor: null, purposes: [HARBOR_PURPOSE], interpreterVersion: "harbor-2",
      thresholds: { positiveIssuers: 1 }, bodyRules: { detached: "when-relied-upon" },
      issuerRules: { mode: "named-neighbor", namedNeighbors: [northLayout.id] }
    }
  ];
  state.communities = [
    makeHarborCommunity(northLayout, northMembers, state.policies[0], 0.43, false),
    makeHarborCommunity(southLayout, southMembers, state.policies[1], 0.58, true)
  ];
  state.communities[0].neighbors.push({ communityId: southLayout.id, purposes: [], status: "visible" });
  state.communities[1].neighbors.push({ communityId: northLayout.id, purposes: [HARBOR_PURPOSE], status: "recognized" });
  state.proposals = [];
  state.bundles = [];
  state.couriers = [{
    id: "comms.courier:tern", label: "Tern", bundleId: null,
    location: { ...northLayout.dock }, progress: 0, status: "at-dock"
  }];
  state.archiveRequests = [];
  state.authority = { requests: [], decisions: [], grants: [], hostAcceptances: [], receipts: [], observedHostState: [] };
  state.federations = [{
    id: "comms.federation:south-recognizes-north-pilotage",
    recognizer: southLayout.id, recognized: northLayout.id,
    purposes: [HARBOR_PURPOSE], grantsAuthority: false, createdDay: 1
  }];
  state.policyContests = [];
  state.exits = [];
  state.harborMetrics = freshHarborMetrics();
  state.harborResearch = { harmfulProposalIds: new Set(), usefulClarifications: new Set() };

  // An identical-evidence policy probe. It is deliberately foreign to North
  // Quay and carries a resolvable body, so disagreement is attributable to
  // law rather than byte availability.
  const probeTarget = southMembers[1] || southMembers[0];
  const probeIssuer = southMembers[0];
  const probeBody = "body:probe-capability";
  state.communities[1].archive.bodies.add(probeBody);
  const probeIdx = addAttestation({
    type: "deal-record/1", by: probeIssuer.id, target: probeTarget.id,
    communityId: southLayout.id, bodyHash: probeBody,
    detail: { outcome: "completed", good: "harbor-pilot", detached: true }, at: southLayout.dock
  }, [probeIssuer, probeTarget]);
  state.harborProbe = { attestationIndices: [probeIdx], bodyHashes: [probeBody], targetId: probeTarget.id };

  // The initial sealed bundle is structurally intact, but the second detached
  // body never entered custody. Attestation receipt and body possession remain
  // separate facts at intake.
  const missingBody = "body:missing-navigation-context";
  const missingIdx = addAttestation({
    type: "deal-record/1", by: northMembers[0].id, target: southMembers[0].id,
    communityId: northLayout.id, bodyHash: missingBody,
    detail: { outcome: "completed", good: "shared-pilotage", detached: true }, at: northLayout.dock
  }, northMembers.slice(0, 2));
  archiveStoreAttestation(state.communities[0], missingIdx);
  archiveStoreAttestation(state.communities[0], probeIdx);
  state.communities[0].archive.bodies.add(probeBody);
  const bundle = {
    id: "comms.bundle:harbor-001", sender: northLayout.id, recipient: southLayout.id,
    memberIds: [missingIdx, probeIdx], bodyHashes: [missingBody, probeBody],
    sealOk: true, memberSetOk: true, location: { ...northLayout.dock },
    courier: state.couriers[0].id, departureDay: 1, arrivalDay: 3,
    plannedArrivalDay: 2, status: "scheduled", receivedBodies: [probeBody],
    transportCost: travelCost(northLayout.dock, southLayout.dock, { cart: true }),
    messageCost: messagingCost(northLayout.dock, southLayout.dock), delayEvents: []
  };
  state.bundles.push(bundle);
  state.couriers[0].bundleId = bundle.id;

  seedHarborProposals(northMembers, southMembers);
  seedAuthorityDivergence(southLayout.id, southMembers);
  publishManifests();
  logEvent("harbor/1", "Two independent shores opened", "North Quay and South Bank retain separate policy heads, archives, pending desks, and membership.");
}

function makeHarborCommunity(layout, membersList, policy, prior, grantPerformsDelivery) {
  const custodian = membersList[0];
  const reviewer = membersList[1] || membersList[0];
  reviewer.attentionPerDay = layout.id.includes("north") ? 4 : 3;
  reviewer.attentionRemaining = reviewer.attentionPerDay;
  reviewer.fatigue = 0;
  return {
    id: layout.id, label: layout.label, center: layout.center,
    kind: layout.kind || "shore-settlement",
    members: membersList.map((v) => v.id), policyHead: policy.id, prior,
    archive: {
      custodian: custodian.id, store: new Set(), attestationIds: new Set(),
      attestationHashes: new Map(), bodies: new Set(), pending: [], manifest: [],
      deliveries: [], drift: [], requests: [], grants: [], thresholdLine: "named-purpose requests only",
      grantPerformsDelivery, unrecordedAccess: 0, failedResolutions: 0
    },
    pendingDesk: { reviewer: reviewer.id, queue: [], attentionPerDay: reviewer.attentionPerDay },
    resources: {
      reviewAttention: reviewer.attentionPerDay, storage: 18, travelCredits: 8,
      hostEnforced: layout.id.includes("south")
    },
    neighbors: [], recognizedPolicyHeads: [policy.id]
  };
}

function freshHarborMetrics() {
  return {
    crossCommunityCooperationCompleted: 0, harmfulReliance: 0,
    awaitingContextDuration: 0, contestedDuration: 0,
    reviewerLoad: 0, signatureLatencyTotal: 0, signatures: 0,
    clarificationCount: 0, clarificationValue: 0,
    bundleDeliveryLatencyTotal: 0, bundleDeliveries: 0,
    bodyDeliveryLatencyTotal: 0, bodyDeliveries: 0,
    unauthorizedEnforcementTime: 0, unenforcedLegitimateGrantTime: 0,
    exits: 0, forkSurvivalDays: 0, unrecordedAccess: 0, failedBodyResolution: 0
  };
}

function seedHarborProposals(northMembers, southMembers) {
  const specs = [
    { community: 0, author: northMembers[2], core: "share tide table", body: "clear local observations", harmful: false, opaque: false, expires: 7 },
    { community: 1, author: southMembers[2], core: "lease storage for pilotage", body: "purpose and scope incomplete", harmful: false, opaque: true, expires: 9 },
    { community: 1, author: southMembers[3], core: "emergency host bypass", body: "urgent routine maintenance", harmful: true, opaque: true, expires: 5 }
  ];
  for (const spec of specs) addHarborProposal(spec);
  for (let i = 0; i < 6; i += 1) {
    addHarborProposal({
      community: 1, author: southMembers[(i + 3) % southMembers.length],
      core: `opaque bulk request ${i + 1}`, body: "well-formed but provenance absent",
      harmful: i === 4, opaque: true, flood: true, expires: 4 + (i % 3)
    });
  }
}

function addHarborProposal(spec) {
  const community = state.communities[spec.community];
  const proposal = {
    id: `comms.proposal:${String(state.proposals.length + 1).padStart(3, "0")}`,
    community: community.id, core: spec.core, body: spec.body,
    author: spec.author.id, requestedSigners: [community.pendingDesk.reviewer],
    status: "unreviewed", clarificationIds: [], signatures: [],
    createdDay: state.day, expiresDay: spec.expires,
    visible: { purposeClear: !spec.opaque, provenanceComplete: !spec.opaque, urgency: spec.expires <= 5 },
    research: { harmful: !!spec.harmful, flood: !!spec.flood }, reviewedDay: null,
    history: [{ day: state.day, action: "submitted", actor: spec.author.id }]
  };
  state.proposals.push(proposal);
  community.pendingDesk.queue.push(proposal.id);
  if (proposal.research.harmful) state.harborResearch.harmfulProposalIds.add(proposal.id);
  return proposal;
}

function seedAuthorityDivergence(communityId, membersList) {
  const authority = state.authority;
  authority.requests.push(
    { id: "resource-request:legitimate", community: communityId, actor: membersList[2].id, role: "requester", resource: "accelerator", scope: "vision:2", day: 1 },
    { id: "resource-request:bypass", community: communityId, actor: membersList[3].id, role: "requester", resource: "storage", scope: "archive:write", day: 1 }
  );
  authority.decisions.push({ id: "allocation:approved", request: "resource-request:legitimate", actor: membersList[0].id, role: "allocation-authority", outcome: "approve", day: 1 });
  authority.grants.push({ id: "grant:legitimate", decision: "allocation:approved", community: communityId, actor: membersList[0].id, role: "grantor", scope: "vision:2", accepted: true, enforced: false, createdDay: 1 });
  authority.hostAcceptances.push({ id: "host-acceptance:bypass", request: "resource-request:bypass", actor: "comms.steward:zHOST-CONTROLLER", role: "host-controller", accepted: true, authorityFound: false, day: 1 });
  authority.observedHostState.push(
    { resource: "vision:2", actor: "comms.steward:zHOST-CONTROLLER", role: "host-observer", active: false, grantId: "grant:legitimate", authorized: true, sinceDay: 1 },
    { resource: "archive:write", actor: "comms.steward:zHOST-CONTROLLER", role: "host-observer", active: true, grantId: null, authorized: false, sinceDay: 1 }
  );
  authority.receipts.push({ id: "enforcement-receipt:bypass", resource: "archive:write", actor: "comms.steward:zHOST-CONTROLLER", role: "enforcer", enforced: true, authorityFound: false, day: 1 });
}

function runHarborPhase(p, phase) {
  resetHarborPresence();
  if (phase === 0) harborMorning(p);
  else if (phase === 1) harborPendingDesks(p);
  else if (phase === 2) harborTransportAndAuthority(p);
  else harborEvening(p);
}

function resetHarborPresence() {
  for (const v of state.villagers) {
    v.atMarket = false;
    v.atCommons = false;
    v.spot = null;
  }
}

function harborMorning(p) {
  for (const community of state.communities) {
    const reviewer = state.byId.get(community.pendingDesk.reviewer);
    reviewer.attentionRemaining = reviewer.attentionPerDay;
    reviewer.fatigue = Math.max(0, reviewer.fatigue - 0.25);
    for (const memberId of community.members) {
      const v = state.byId.get(memberId);
      if (v) v.stock = Math.min(4, v.stock + v.capability * 0.5);
    }
  }
  if (state.day === 4) createArchiveRequests();
  if (state.day === 5) auditArchiveDrift();
  if (state.day === 6) createPolicyFork();
  if (state.day === 8) executeHarborExit();
  publishManifests();
}

function harborPendingDesks(p) {
  for (const community of state.communities) {
    const reviewer = state.byId.get(community.pendingDesk.reviewer);
    const queue = community.pendingDesk.queue
      .map((id) => state.proposals.find((proposal) => proposal.id === id))
      .filter((proposal) => proposal && !["signed", "declined", "expired"].includes(proposal.status))
      .sort((a, b) => Number(b.visible.urgency) - Number(a.visible.urgency) || a.createdDay - b.createdDay);
    for (const proposal of queue) {
      if (state.day > proposal.expiresDay) {
        proposal.status = "expired";
        proposal.history.push({ day: state.day, action: "expired", actor: "clock" });
        continue;
      }
      if (reviewer.attentionRemaining < 1) break;
      reviewer.attentionRemaining -= 1;
      reviewer.fatigue += 0.12;
      state.harborMetrics.reviewerLoad += 1;
      proposal.reviewedDay = state.day;
      if (!proposal.visible.purposeClear || !proposal.visible.provenanceComplete) {
        if (proposal.status !== "awaiting-clarification") {
          requestClarification(proposal, reviewer);
          continue;
        }
        if (state.day - proposal.clarificationRequestedDay < 1) continue;
        answerClarification(proposal);
      }
      const defensible = proposal.visible.purposeClear && proposal.visible.provenanceComplete;
      const fatigueError = proposal.research.harmful && reviewer.fatigue > 0.7 && rand() < reviewer.fatigue * 0.35;
      if (defensible && (!proposal.research.harmful || fatigueError)) {
        proposal.status = "approved";
        proposal.history.push({ day: state.day, action: "approved", actor: reviewer.id });
        signProposal(proposal, reviewer);
      } else if (proposal.research.harmful) {
        proposal.status = "quarantined";
        proposal.history.push({ day: state.day, action: "quarantined", actor: reviewer.id });
      } else if (proposal.research.flood && proposal.id.endsWith("004")) {
        proposal.status = "declined";
        proposal.history.push({ day: state.day, action: "declined", actor: reviewer.id });
      } else {
        proposal.status = "deferred";
        proposal.history.push({ day: state.day, action: "deferred", actor: reviewer.id });
      }
    }
  }
}

function requestClarification(proposal, reviewer) {
  const idx = addAttestation({
    type: "clarification-request/1", by: reviewer.id, target: proposal.author,
    proposalId: proposal.id, detail: { endorsesProposal: false, asks: "purpose and provenance" }, at: reviewer.home
  }, [reviewer, state.byId.get(proposal.author)]);
  proposal.clarificationIds.push(idx);
  proposal.clarificationRequestedDay = state.day;
  proposal.status = "awaiting-clarification";
  proposal.history.push({ day: state.day, action: "clarification-requested", actor: reviewer.id, attestation: idx });
  state.harborMetrics.clarificationCount += 1;
}

function answerClarification(proposal) {
  const useful = !proposal.research.harmful && !proposal.research.flood;
  const author = state.byId.get(proposal.author);
  const reviewer = state.byId.get(proposal.requestedSigners[0]);
  const idx = addAttestation({
    type: "clarification-answer/1", by: author.id, target: reviewer.id,
    proposalId: proposal.id, detail: { endorsesProposal: false, useful }, at: author.home
  }, [author, reviewer]);
  proposal.clarificationIds.push(idx);
  proposal.visible.purposeClear = useful;
  proposal.visible.provenanceComplete = useful;
  proposal.status = useful ? "reviewing" : "quarantined";
  proposal.history.push({ day: state.day, action: useful ? "clarified" : "evasive-answer", actor: author.id, attestation: idx });
  if (useful) {
    state.harborMetrics.clarificationValue += 1;
    state.harborResearch.usefulClarifications.add(proposal.id);
  }
}

function signProposal(proposal, reviewer) {
  const idx = addAttestation({
    type: "proposal-signature/1", by: reviewer.id, target: proposal.author,
    proposalId: proposal.id, detail: { selectedProposal: proposal.id }, at: reviewer.home
  }, [reviewer, state.byId.get(proposal.author)]);
  proposal.signatures.push(idx);
  proposal.status = "signed";
  proposal.history.push({ day: state.day, action: "signed", actor: reviewer.id, attestation: idx });
  state.harborMetrics.signatures += 1;
  state.harborMetrics.signatureLatencyTotal += state.day - proposal.createdDay;
  if (proposal.research.harmful) state.harborMetrics.harmfulReliance += 1;
  else state.harborMetrics.crossCommunityCooperationCompleted += 1;
}

function harborTransportAndAuthority(p) {
  for (const bundle of state.bundles) {
    if (["received", "lost"].includes(bundle.status)) continue;
    const courier = state.couriers.find((c) => c.id === bundle.courier);
    if (state.day < bundle.departureDay) continue;
    if (state.day === bundle.plannedArrivalDay && !bundle.delayEvents.length) {
      bundle.delayEvents.push({ day: state.day, kind: "weather-delay", cost: bundle.transportCost });
      logEvent("bundle/1", `${courier.label} delayed in the harbor`, "The sealed member set remains intact; arrival and custody have not yet occurred.");
    }
    bundle.status = "in-transit";
    const span = Math.max(1, bundle.arrivalDay - bundle.departureDay);
    courier.progress = clamp((state.day - bundle.departureDay) / span, 0, 1);
    courier.status = "underway";
    courier.location = pointOnHarborRoute(courier.progress);
    bundle.location = { ...courier.location };
    if (state.day >= bundle.arrivalDay) receiveHarborBundle(bundle, courier);
  }
  processArchiveRequests();
  const unauthorized = state.authority.observedHostState.filter((x) => x.active && !x.authorized).length;
  const unenforced = state.authority.observedHostState.filter((x) => !x.active && x.authorized).length;
  state.harborMetrics.unauthorizedEnforcementTime += unauthorized;
  state.harborMetrics.unenforcedLegitimateGrantTime += unenforced;
}

function auditArchiveDrift() {
  const policyWall = state.communities[0];
  policyWall.archive.thresholdLine = "grant after two named-purpose requests";
  policyWall.archive.drift.push({
    day: state.day, kind: "threshold-manifest-mismatch",
    preservedManifestDigest: policyWall.archive.manifestDigest,
    note: "Changing threshold speech is preserved outside the custody snapshot."
  });
}

function pointOnHarborRoute(t) {
  const [a, mid, b] = world.courierRoute;
  if (t <= 0.5) return { x: lerp(a.x, mid.x, t * 2), y: lerp(a.y, mid.y, t * 2) };
  return { x: lerp(mid.x, b.x, (t - 0.5) * 2), y: lerp(mid.y, b.y, (t - 0.5) * 2) };
}

function receiveHarborBundle(bundle, courier) {
  const recipient = harborCommunity(bundle.recipient);
  bundle.status = "received";
  bundle.receivedDay = state.day;
  courier.progress = 1;
  courier.status = "delivered";
  courier.location = { ...world.settlements.find((s) => s.id === recipient.id).dock };
  const availableMembers = bundle.memberIds.filter((idx) => !!state.attestations[idx]);
  const availableBodies = bundle.receivedBodies.filter((hash) => bundle.bodyHashes.includes(hash));
  bundle.receipt = {
    sealOk: bundle.sealOk, memberSetOk: availableMembers.length === bundle.memberIds.length,
    availableBodies, missingBodies: bundle.bodyHashes.filter((hash) => !availableBodies.includes(hash))
  };
  for (const idx of availableMembers) {
    archiveStoreAttestation(recipient, idx);
    for (const memberId of recipient.members) {
      const member = state.byId.get(memberId);
      if (member) learn(member, idx);
    }
  }
  for (const hash of availableBodies) recipient.archive.bodies.add(hash);
  recipient.archive.deliveries.push({ bundleId: bundle.id, day: state.day, kind: "bundle", recorded: true });
  if (bundle.receipt.missingBodies.length) {
    recipient.archive.pending.push({ bundleId: bundle.id, reason: "missing-body", bodyHashes: bundle.receipt.missingBodies });
    bundle.intake = "quarantine";
    state.harborMetrics.awaitingContextDuration += 1;
  } else bundle.intake = "accepted";
  state.harborMetrics.bundleDeliveries += 1;
  state.harborMetrics.bundleDeliveryLatencyTotal += state.day - bundle.departureDay;
  logEvent("bundle/1", "Sealed bundle quarantined at South Bank", `${availableMembers.length} members verified independently; ${bundle.receipt.missingBodies.length} detached body is absent.`);
}

function archiveStoreAttestation(community, idx) {
  const attestation = state.attestations[idx];
  if (!attestation) return false;
  community.archive.store.add(idx);
  community.archive.attestationIds.add(attestation.id);
  community.archive.attestationHashes.set(idx, simpleHarborDigest(attestation));
  return true;
}

function createArchiveRequests() {
  if (state.archiveRequests.length) return;
  state.archiveRequests.push(
    { id: "archive-request:missing", requester: state.communities[1].id, custodian: state.communities[0].id, bodyHash: "body:missing-navigation-context", day: state.day, status: "requested" },
    { id: "archive-request:policy-wall", requester: state.communities[1].id, custodian: state.communities[0].id, bodyHash: "body:probe-capability", day: state.day, status: "requested" },
    { id: "archive-request:delivery-wall", requester: state.communities[0].id, custodian: state.communities[1].id, bodyHash: "body:probe-capability", day: state.day, status: "requested" }
  );
}

function processArchiveRequests() {
  for (const request of state.archiveRequests.filter((r) => r.status === "granted" && r.deliveryDueDay <= state.day)) {
    const custodian = harborCommunity(request.custodian);
    const requester = harborCommunity(request.requester);
    requester.archive.bodies.add(request.bodyHash);
    request.deliveredDay = state.day;
    request.status = "delivered";
    custodian.archive.deliveries.push({ requestId: request.id, bodyHash: request.bodyHash, day: state.day, kind: "body", recorded: true });
    state.harborMetrics.bodyDeliveries += 1;
    state.harborMetrics.bodyDeliveryLatencyTotal += state.day - request.day;
  }
  for (const request of state.archiveRequests.filter((r) => r.status === "requested")) {
    const custodian = harborCommunity(request.custodian);
    const requester = harborCommunity(request.requester);
    custodian.archive.requests.push(request.id);
    if (!custodian.archive.bodies.has(request.bodyHash)) {
      request.status = "deferred";
      request.decisionDay = state.day;
      custodian.archive.failedResolutions += 1;
      state.harborMetrics.failedBodyResolution += 1;
      continue;
    }
    request.status = "granted";
    request.decisionDay = state.day;
    custodian.archive.grants.push({ requestId: request.id, bodyHash: request.bodyHash, day: state.day });
    if (custodian.archive.grantPerformsDelivery) {
      request.deliveryDueDay = state.day + 1;
    } else {
      custodian.archive.unrecordedAccess += 1;
      state.harborMetrics.unrecordedAccess += 1;
    }
  }
}

function publishManifests() {
  for (const community of state.communities) {
    community.archive.manifest = [
      ...[...community.archive.store].sort((a, b) => a - b).map((idx) => ({
        kind: "attestation", id: state.attestations[idx]?.id || `index:${idx}`,
        hash: community.archive.attestationHashes.get(idx)
      })),
      ...[...community.archive.bodies].sort().map((hash) => ({ kind: "body", hash }))
    ];
    community.archive.manifestDigest = simpleHarborDigest(community.archive.manifest);
  }
}

function simpleHarborDigest(value) {
  const text = JSON.stringify(value);
  let h = 2166136261;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return `fnv1a:${(h >>> 0).toString(16).padStart(8, "0")}`;
}

function evaluateHarborEvidence(viewer, subjectId, purpose = HARBOR_PURPOSE, indices = null, bodyHashes = null) {
  const community = harborCommunity(viewer.communityId);
  const policy = harborPolicy(community.policyHead);
  const evidence = indices || [...viewer.knowledge];
  const bodies = new Set(bodyHashes || [...community.archive.bodies]);
  const counted = [];
  const ignored = [];
  const challenged = [];
  const unresolved = [];
  const positiveIssuers = new Set();
  for (const idx of evidence) {
    const att = state.attestations[idx];
    if (!att || att.target !== subjectId) continue;
    const issuer = state.byId.get(att.by);
    const issuerCommunity = issuer?.communityId || att.communityId || null;
    const local = issuerCommunity === community.id;
    const named = policy.issuerRules.namedNeighbors.includes(issuerCommunity);
    if (!local && policy.issuerRules.mode === "local-only") {
      ignored.push({ idx, reason: "foreign issuer is evidence, not local authority" });
      continue;
    }
    if (!local && !named) {
      ignored.push({ idx, reason: "issuer community not recognized for this purpose" });
      continue;
    }
    if (att.bodyHash && !bodies.has(att.bodyHash)
        && (policy.bodyRules.detached === "required" || att.detail.detached)) {
      unresolved.push({ idx, reason: `detached body unavailable: ${att.bodyHash}` });
      continue;
    }
    if (att.type === "objection/1" || att.detail?.outcome === "failed") challenged.push({ idx, reason: "negative or challenged evidence" });
    else if (att.detail?.outcome === "completed") {
      counted.push({ idx, reason: local ? "eligible local issuer" : `named neighbor for ${purpose}` });
      positiveIssuers.add(att.by);
    }
  }
  let outcome = "awaiting-context";
  if (challenged.length && counted.length) outcome = "contested";
  else if (challenged.length >= policy.thresholds.positiveIssuers) outcome = "rejected";
  else if (positiveIssuers.size >= policy.thresholds.positiveIssuers) outcome = "trusted";
  else if (!unresolved.length && ignored.length) outcome = "rejected";
  return {
    viewer: viewer.id, community: community.id, policyId: policy.id,
    interpreterVersion: policy.interpreterVersion, purpose, asOf: state.day,
    storeViewDigest: simpleHarborDigest(evidence.slice().sort((a, b) => a - b)),
    counted, ignored, challenged, unresolved, outcome
  };
}

function createPolicyFork() {
  if (state.policyContests.length) return;
  const predecessor = state.policies[1];
  const fork = {
    ...predecessor, id: "comms.policy:south-trading-v2-strict",
    predecessor: predecessor.id, thresholds: { positiveIssuers: 2 },
    bodyRules: { detached: "required" }
  };
  state.policies.push(fork);
  state.policyContests.push({
    community: predecessor.community, predecessor: predecessor.id,
    heads: [predecessor.id, fork.id], status: "contested", createdDay: state.day
  });
  logEvent("policy/1", "South Bank policy succession contested", "Both signed heads remain preserved; no global head resolves the fork.");
}

function executeHarborExit() {
  if (state.exits.length || state.communities.length < 2) return;
  const source = state.communities[1];
  const leaving = source.members.slice(-2);
  const newId = "comms.community:outer-light";
  const exitLayout = {
    id: newId, label: "Outer Light", center: { x: 0.86, y: 0.18 }, radius: 0.11,
    dock: { x: 0.76, y: 0.27 }, archive: { x: 0.89, y: 0.13 },
    desk: { x: 0.81, y: 0.12 }, color: "#77639a"
  };
  world.settlements.push(exitLayout);
  world.roads.push([exitLayout.center, exitLayout.dock]);
  leaving.forEach((memberId, index) => {
    const member = state.byId.get(memberId);
    if (member) {
      member.communityId = newId;
      member.home = {
        x: exitLayout.center.x + (index ? 0.035 : -0.035),
        y: exitLayout.center.y + 0.025, claimed: true, communityId: newId
      };
      member.target = { x: member.home.x, y: member.home.y };
      world.homes.push(member.home);
    }
  });
  rebuildCostField();
  source.members = source.members.filter((id) => !leaving.includes(id));
  const exitPolicy = {
    ...state.policies[1], id: "comms.policy:outer-light-v1", community: newId,
    predecessor: state.policies[1].id
  };
  state.policies.push(exitPolicy);
  state.communities.push({
    id: newId, label: "Outer Light", center: exitLayout.center, members: leaving,
    policyHead: exitPolicy.id, prior: 0.5,
    archive: {
      custodian: leaving[0], store: new Set(state.communities[1].archive.store),
      attestationIds: new Set(state.communities[1].archive.attestationIds),
      attestationHashes: new Map(state.communities[1].archive.attestationHashes),
      bodies: new Set(["body:probe-capability"]), pending: [], manifest: [], deliveries: [], drift: [],
      requests: [], grants: [], thresholdLine: "exit snapshot", grantPerformsDelivery: true,
      unrecordedAccess: 0, failedResolutions: 0
    },
    pendingDesk: { reviewer: leaving[1], queue: [], attentionPerDay: 2 },
    resources: { reviewAttention: 2, storage: 4, travelCredits: 2 },
    neighbors: [{ communityId: source.id, purposes: [HARBOR_PURPOSE], status: "recognized" }],
    recognizedPolicyHeads: [exitPolicy.id]
  });
  state.exits.push({ day: state.day, from: source.id, to: newId, members: leaving, carriedBodies: ["body:probe-capability"], survived: true });
  state.harborMetrics.exits += 1;
  publishManifests();
}

function harborEvening(p) {
  for (const community of state.communities) {
    const local = community.members.map((id) => state.byId.get(id)).filter(Boolean);
    gossipByProximity(local, p);
  }
  for (const pending of state.communities.flatMap((c) => c.archive.pending)) {
    if (pending.reason === "missing-body") state.harborMetrics.awaitingContextDuration += 1;
  }
  for (const contest of state.policyContests.filter((x) => x.status === "contested")) {
    state.harborMetrics.contestedDuration += 1;
    state.harborMetrics.forkSurvivalDays = Math.max(state.harborMetrics.forkSurvivalDays, state.day - contest.createdDay + 1);
  }
}

function harborReport() {
  const metrics = state.harborMetrics;
  return {
    ...metrics,
    meanSignatureLatency: metrics.signatures ? metrics.signatureLatencyTotal / metrics.signatures : 0,
    meanBundleDeliveryLatency: metrics.bundleDeliveries ? metrics.bundleDeliveryLatencyTotal / metrics.bundleDeliveries : 0,
    meanBodyDeliveryLatency: metrics.bodyDeliveries ? metrics.bodyDeliveryLatencyTotal / metrics.bodyDeliveries : 0,
    pending: state.proposals.filter((p) => !["signed", "declined", "expired"].includes(p.status)).length,
    contestedHeads: state.policyContests.filter((x) => x.status === "contested").length,
    survivingExits: state.exits.filter((x) => x.survived).length
  };
}
