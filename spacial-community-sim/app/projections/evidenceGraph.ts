import type { EvidenceGraphProjection } from "../models/projections";
import type { LegacyHarborSnapshot } from "../models/snapshot";

export function projectEvidenceGraph(snapshot: LegacyHarborSnapshot): EvidenceGraphProjection {
  const nodes: EvidenceGraphProjection["nodes"] = [];
  const edges: EvidenceGraphProjection["edges"] = [];
  const nodeIds = new Set<string>();
  const addNode = (node: EvidenceGraphProjection["nodes"][number]) => {
    if (nodeIds.has(node.id)) return;
    nodeIds.add(node.id);
    nodes.push(node);
  };

  for (const community of snapshot.communities) {
    const layout = snapshot.settlements.find((settlement) => settlement.id === community.id);
    addNode({
      id: community.id, kind: "community", label: community.label,
      position: layout?.center, color: layout?.color ?? "#637074",
      opacity: layout?.recognized === false ? 0.28 : 1
    });
    for (const bodyHash of community.bodyHashes) {
      const bodyId = bodyHash;
      addNode({ id: bodyId, kind: "body", label: bodyHash.replace(/^body:/, ""), color: "#b87815", opacity: 1 });
      edges.push({
        id: `custody:${community.id}:${bodyHash}`, kind: "custody",
        source: community.id, target: bodyId, challenged: false, label: "holds"
      });
    }
  }
  for (const resident of snapshot.residents) {
    addNode({
      id: resident.id, kind: "resident", label: resident.label,
      position: resident.position, color: "#4f8f8b",
      opacity: resident.judgment.kind === "stranger" ? 0.35 : 1
    });
    if (resident.communityId && nodeIds.has(resident.communityId)) {
      edges.push({
        id: `membership:${resident.id}:${resident.communityId}`, kind: "membership",
        source: resident.id, target: resident.communityId, challenged: false
      });
    }
  }
  for (const attestation of snapshot.attestations) {
    if (!attestation.target || !nodeIds.has(attestation.by) || !nodeIds.has(attestation.target)) continue;
    edges.push({
      id: `attestation:${attestation.id}`, kind: "attestation",
      source: attestation.by, target: attestation.target,
      challenged: attestation.challenged, label: attestation.type.replace("/1", "")
    });
  }
  return {
    schemaVersion: 1,
    kind: "evidence-graph",
    asOf: { day: snapshot.day, phase: snapshot.phase },
    viewerId: snapshot.viewer?.id ?? null,
    nodes,
    edges
  };
}
