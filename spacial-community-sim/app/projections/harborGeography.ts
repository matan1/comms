import type { HarborGeographyProjection } from "../models/projections";
import type { JudgmentKind, LegacyHarborSnapshot } from "../models/snapshot";

const JUDGMENT_COLORS: Record<JudgmentKind, number> = {
  omniscient: 0x6a8f5a,
  self: 0x2e6f9e,
  trusted: 0x2f7d66,
  rejected: 0xb65345,
  contested: 0x8a5a9c,
  "awaiting-context": 0x9aa39b,
  stranger: 0x9aa39b
};

export function projectHarborGeography(snapshot: LegacyHarborSnapshot): HarborGeographyProjection {
  if (snapshot.schemaVersion !== 1 || snapshot.worldKind !== "harbor") {
    throw new Error("Unsupported Harbor snapshot.");
  }
  const opacityByCommunity = new Map(snapshot.settlements.map((settlement) => [
    settlement.id,
    settlement.recognized ? 1 : 0.28
  ]));
  return {
    schemaVersion: 1,
    kind: "harbor-geography",
    asOf: { day: snapshot.day, phase: snapshot.phase },
    viewerId: snapshot.viewer?.id ?? null,
    settlements: snapshot.settlements.map((settlement) => ({
      id: settlement.id,
      label: settlement.label,
      kind: settlement.kind,
      center: settlement.center,
      radius: settlement.radius,
      boundaryColor: settlement.color,
      opacity: opacityByCommunity.get(settlement.id) ?? 0.28,
      stations: [
        { kind: "dock", position: settlement.dock, label: "DOCK" },
        { kind: "archive", position: settlement.archive, label: "ARCHIVE" },
        { kind: "pending", position: settlement.desk, label: "PENDING" }
      ]
    })),
    homes: snapshot.homes.map((home) => ({
      ...home,
      opacity: home.communityId ? opacityByCommunity.get(home.communityId) ?? 0.28 : 0.35
    })),
    residents: snapshot.residents.map((resident) => ({
      id: resident.id,
      label: resident.label,
      communityId: resident.communityId,
      position: resident.position,
      radius: 5 + resident.capability * 4,
      fill: JUDGMENT_COLORS[resident.judgment.kind],
      opacity: resident.judgment.kind === "stranger" || resident.judgment.kind === "awaiting-context" ? 0.45 : 1,
      judgment: resident.judgment.kind,
      selected: snapshot.viewer?.id === resident.id
    })),
    route: snapshot.route,
    couriers: snapshot.couriers.map((courier) => ({ ...courier }))
  };
}

