import { describe, expect, it } from "vitest";
import { projectEvidenceGraph } from "../app/projections/evidenceGraph";
import { projectHarborGeography } from "../app/projections/harborGeography";
import { harborSnapshot } from "./fixtures";

describe("Harbor geography projection", () => {
  it("is deterministic and dims only unrecognized geography", () => {
    const snapshot = harborSnapshot();
    const first = projectHarborGeography(snapshot);
    const second = projectHarborGeography(structuredClone(snapshot));
    expect(second).toEqual(first);
    expect(first.settlements.map((settlement) => settlement.opacity)).toEqual([1, 0.28]);
    expect(first.homes.map((home) => home.opacity)).toEqual([1, 0.28]);
  });

  it("preserves semantic selection, movement, and transport without raw state", () => {
    const projection = projectHarborGeography(harborSnapshot());
    expect(projection.residents.find((resident) => resident.selected)?.id).toBe("north:viewer");
    expect(projection.residents.find((resident) => resident.id === "south:subject")?.opacity).toBe(0.45);
    expect(projection.couriers[0]).toMatchObject({ status: "underway", progress: 0.5 });
    expect(JSON.stringify(projection)).not.toContain("research");
  });
});

describe("Evidence graph projection", () => {
  it("models membership, custody, and attestations as distinct edges", () => {
    const projection = projectEvidenceGraph(harborSnapshot());
    expect(new Set(projection.edges.map((edge) => edge.kind))).toEqual(new Set(["membership", "custody", "attestation"]));
    expect(projection.nodes.find((node) => node.kind === "body")?.id).toBe("body:known");
  });

  it("does not invent nodes for unresolved references", () => {
    const snapshot = harborSnapshot();
    snapshot.attestations.push({ idx: 2, id: "att:orphan", type: "deal-record/1", by: "absent", target: "south:subject", bodyHash: null, challenged: false });
    const projection = projectEvidenceGraph(snapshot);
    expect(projection.edges.some((edge) => edge.id === "attestation:att:orphan")).toBe(false);
  });
});
