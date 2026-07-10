import type { LegacyHarborSnapshot } from "../app/models/snapshot";

export function harborSnapshot(): LegacyHarborSnapshot {
  return {
    schemaVersion: 1,
    worldKind: "harbor",
    projectionMode: "geography",
    day: 3,
    phase: 2,
    viewer: { id: "north:viewer", communityId: "north", label: "Wren" },
    settlements: [
      { id: "north", label: "North", kind: "shore-settlement", center: { x: 0.2, y: 0.3 }, radius: 0.2, dock: { x: 0.4, y: 0.4 }, archive: { x: 0.1, y: 0.2 }, desk: { x: 0.3, y: 0.2 }, color: "#477f96", recognized: true },
      { id: "south", label: "South", kind: "workstation-settlement", center: { x: 0.8, y: 0.7 }, radius: 0.2, dock: { x: 0.6, y: 0.6 }, archive: { x: 0.9, y: 0.8 }, desk: { x: 0.7, y: 0.8 }, color: "#9b6b49", recognized: false }
    ],
    homes: [
      { position: { x: 0.15, y: 0.3 }, communityId: "north", occupied: true },
      { position: { x: 0.85, y: 0.7 }, communityId: "south", occupied: true }
    ],
    residents: [
      { id: "north:viewer", label: "Wren", communityId: "north", member: true, capability: 0.8, specialty: "grain", position: { x: 0.2, y: 0.3 }, home: { x: 0.2, y: 0.3 }, judgment: { kind: "self", trust: 0.6 } },
      { id: "south:subject", label: "Sage", communityId: "south", member: true, capability: 0.9, specialty: "tools", position: { x: 0.8, y: 0.7 }, home: { x: 0.8, y: 0.7 }, judgment: { kind: "stranger", trust: null } }
    ],
    route: [{ x: 0.4, y: 0.4 }, { x: 0.5, y: 0.5 }, { x: 0.6, y: 0.6 }],
    couriers: [{ id: "courier", label: "Tern", status: "underway", bundleId: "bundle", position: { x: 0.5, y: 0.5 }, progress: 0.5 }],
    communities: [
      { id: "north", label: "North", kind: "shore-settlement", policyHead: "north-v1", memberIds: ["north:viewer"], bodyHashes: ["body:known"], attestationIndices: [1] },
      { id: "south", label: "South", kind: "workstation-settlement", policyHead: "south-v1", memberIds: ["south:subject"], bodyHashes: [], attestationIndices: [] }
    ],
    attestations: [{ idx: 1, id: "att:1", type: "deal-record/1", by: "north:viewer", target: "south:subject", bodyHash: "body:known", challenged: false }]
  };
}

