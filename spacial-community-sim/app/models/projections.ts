import type { JudgmentKind, Point } from "./snapshot";

export interface HarborGeographyProjection {
  schemaVersion: 1;
  kind: "harbor-geography";
  asOf: { day: number; phase: number };
  viewerId: string | null;
  settlements: Array<{
    id: string;
    label: string;
    kind: string;
    center: Point;
    radius: number;
    boundaryColor: string;
    opacity: number;
    stations: Array<{ kind: "dock" | "archive" | "pending"; position: Point; label: string }>;
  }>;
  homes: Array<{ position: Point; communityId: string | null; occupied: boolean; opacity: number }>;
  residents: Array<{
    id: string;
    label: string;
    communityId: string | null;
    position: Point;
    radius: number;
    fill: number;
    opacity: number;
    judgment: JudgmentKind;
    selected: boolean;
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
}

export interface EvidenceGraphProjection {
  schemaVersion: 1;
  kind: "evidence-graph";
  asOf: { day: number; phase: number };
  viewerId: string | null;
  nodes: Array<{
    id: string;
    kind: "community" | "resident" | "body";
    label: string;
    position?: Point;
    color: string;
    opacity: number;
  }>;
  edges: Array<{
    id: string;
    kind: "membership" | "attestation" | "custody";
    source: string;
    target: string;
    challenged: boolean;
    label?: string;
  }>;
}

