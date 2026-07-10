export interface Point {
  x: number;
  y: number;
}

export type JudgmentKind =
  | "omniscient"
  | "self"
  | "trusted"
  | "rejected"
  | "contested"
  | "awaiting-context"
  | "stranger";

export interface LegacyHarborSnapshot {
  schemaVersion: 1;
  worldKind: "harbor";
  projectionMode: string;
  day: number;
  phase: number;
  viewer: { id: string; communityId: string | null; label: string } | null;
  settlements: Array<{
    id: string;
    label: string;
    kind: string;
    center: Point;
    radius: number;
    dock: Point;
    archive: Point;
    desk: Point;
    color: string;
    recognized: boolean;
  }>;
  homes: Array<{ position: Point; communityId: string | null; occupied: boolean }>;
  residents: Array<{
    id: string;
    label: string;
    communityId: string | null;
    member: boolean;
    capability: number;
    specialty: string;
    position: Point;
    home: Point;
    judgment: { kind: JudgmentKind; trust: number | null; adversaryType?: string | null };
  }>;
  route: Point[];
  couriers: Array<{
    id: string;
    label: string;
    status: string;
    bundleId: string | null;
    position: Point;
    progress: number;
  }>;
  communities: Array<{
    id: string;
    label: string;
    kind: string;
    policyHead: string;
    memberIds: string[];
    bodyHashes: string[];
    attestationIndices: number[];
  }>;
  attestations: Array<{
    idx: number;
    id: string;
    type: string;
    by: string;
    target: string | null;
    bodyHash: string | null;
    challenged: boolean;
  }>;
}

