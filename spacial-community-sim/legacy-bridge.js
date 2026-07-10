// Read-only compatibility bridge from the classic-script simulator to the
// typed projection shell. The bridge deliberately exports plain snapshots,
// never the mutable state/world objects themselves.

"use strict";

function bridgeJudgment(viewer, resident) {
  if (!viewer) {
    return {
      kind: "omniscient",
      trust: state.cached.mean.get(resident.id) ?? state.prior,
      adversaryType: resident.adversaryType || null
    };
  }
  if (viewer.id === resident.id) return { kind: "self", trust: state.cached.mean.get(resident.id) ?? state.prior };
  if (params().vouchMode) {
    const result = perceivedVouch(viewer, resident.id);
    return { kind: result.outcome, trust: null };
  }
  if (isStrangerTo(viewer, resident.id)) return { kind: "stranger", trust: null };
  const trust = perceivedTrust(viewer, resident.id);
  return { kind: trust > 0.72 ? "trusted" : trust <= 0.3 ? "rejected" : "contested", trust };
}

function harborShellSnapshot() {
  if (!state || !world || world.kind !== "harbor") return null;
  const viewer = viewpoint();
  const viewerCommunity = viewer ? harborCommunity(viewer.communityId) : null;
  const recognized = new Set([
    viewerCommunity?.id,
    ...(viewerCommunity?.neighbors || [])
      .filter((neighbor) => neighbor.status === "recognized")
      .map((neighbor) => neighbor.communityId)
  ].filter(Boolean));
  const visibleAttestations = viewer
    ? [...viewer.knowledge].map((idx) => state.attestations[idx]).filter(Boolean)
    : state.attestations;

  return {
    schemaVersion: 1,
    worldKind: "harbor",
    projectionMode: harborProjectionMode(),
    day: state.day,
    phase: state.phase,
    viewer: viewer ? { id: viewer.id, communityId: viewer.communityId, label: viewer.label } : null,
    settlements: world.settlements.map((settlement) => ({
      id: settlement.id,
      label: settlement.label,
      kind: settlement.kind || "shore-settlement",
      center: { ...settlement.center },
      radius: settlement.radius,
      dock: { ...settlement.dock },
      archive: { ...settlement.archive },
      desk: { ...settlement.desk },
      color: settlement.color,
      recognized: !viewer || recognized.has(settlement.id)
    })),
    homes: world.homes.map((home) => ({
      position: { x: home.x, y: home.y },
      communityId: home.communityId || null,
      occupied: !!home.claimed
    })),
    residents: state.villagers.map((resident) => ({
      id: resident.id,
      label: resident.label,
      communityId: resident.communityId,
      member: resident.member,
      capability: resident.capability,
      specialty: resident.specialty.id,
      position: { ...resident.pos },
      home: { x: resident.home.x, y: resident.home.y },
      judgment: bridgeJudgment(viewer, resident)
    })),
    route: world.courierRoute.map((point) => ({ ...point })),
    couriers: state.couriers.map((courier) => ({
      id: courier.id,
      label: courier.label,
      status: courier.status,
      bundleId: courier.bundleId,
      position: { ...courier.location },
      progress: courier.progress
    })),
    communities: state.communities.map((community) => ({
      id: community.id,
      label: community.label,
      kind: community.kind,
      policyHead: community.policyHead,
      memberIds: [...community.members],
      bodyHashes: [...community.archive.bodies].sort(),
      attestationIndices: [...community.archive.store].sort((a, b) => a - b)
    })),
    attestations: visibleAttestations.map((attestation) => ({
      idx: attestation.idx,
      id: attestation.id,
      type: attestation.type,
      by: attestation.by,
      target: attestation.target || null,
      bodyHash: attestation.bodyHash || null,
      challenged: attestation.type === "objection/1" || attestation.detail?.outcome === "failed"
    }))
  };
}

globalThis.CommsSimulation = Object.freeze({
    getSnapshot: harborShellSnapshot,
    getView: () => ({
      worldKind: world?.kind || null,
      projectionMode: world?.kind === "harbor" ? harborProjectionMode() : null
    }),
    selectViewer: (id) => {
      selectedId = id && state.byId.has(id) ? id : null;
      if (hasDom) renderStatic();
    },
    subscribe: (listener) => {
      if (!hasDom) return () => {};
      window.addEventListener("comms:statechange", listener);
      return () => window.removeEventListener("comms:statechange", listener);
    }
  });
