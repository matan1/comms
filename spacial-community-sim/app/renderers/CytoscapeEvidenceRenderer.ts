import cytoscape, { type Core, type ElementDefinition, type NodeSingular, type StylesheetJson } from "cytoscape";
import type { SimulationKernel } from "../kernel/SimulationKernel";
import type { EvidenceGraphProjection } from "../models/projections";
import type { ProjectionRenderer } from "./ProjectionRenderer";

export class CytoscapeEvidenceRenderer implements ProjectionRenderer<EvidenceGraphProjection> {
  private core: Core | null = null;
  private container: HTMLElement | null = null;

  public constructor(private readonly kernel: SimulationKernel) {}

  public mount(container: HTMLElement): void {
    this.container = container;
  }

  public render(projection: EvidenceGraphProjection): void {
    if (!this.container) return;
    this.core?.destroy();
    const elements: ElementDefinition[] = [
      ...projection.nodes.map((node, index) => ({
        data: { id: node.id, label: node.label, kind: node.kind, color: node.color, opacity: node.opacity },
        position: node.position
          ? { x: node.position.x * 900, y: node.position.y * 650 }
          : { x: 450 + Math.cos(index) * 260, y: 325 + Math.sin(index) * 220 }
      })),
      ...projection.edges.map((edge) => ({
        data: { ...edge }
      }))
    ];
    const style: StylesheetJson = [
      {
        selector: "node",
        style: {
          "background-color": "data(color)", "label": "data(label)",
          "opacity": (node: NodeSingular) => Number(node.data("opacity")),
          "font-family": "ui-monospace, monospace", "font-size": 11, "text-valign": "bottom",
          "text-margin-y": 7, "color": "#243538", "width": 18, "height": 18
        }
      },
      { selector: 'node[kind = "community"]', style: { "shape": "round-rectangle", "width": 36, "height": 26, "font-weight": "bold" } },
      { selector: 'node[kind = "body"]', style: { "shape": "diamond", "width": 14, "height": 14 } },
      { selector: "edge", style: { "curve-style": "bezier", "width": 1.2, "line-color": "#708080", "target-arrow-color": "#708080", "target-arrow-shape": "triangle", "arrow-scale": 0.65, "opacity": 0.52 } },
      { selector: 'edge[kind = "membership"]', style: { "line-style": "dotted", "target-arrow-shape": "none", "line-color": "#8b8674" } },
      { selector: 'edge[kind = "custody"]', style: { "line-style": "dashed", "line-color": "#b87815", "target-arrow-color": "#b87815" } },
      { selector: "edge[challenged]", style: { "line-color": "#b65345", "target-arrow-color": "#b65345", "width": 2 } },
      { selector: ":selected", style: { "border-width": 3, "border-color": "#1e2527" } }
    ];
    this.core = cytoscape({
      container: this.container,
      elements,
      style,
      layout: { name: "preset", fit: true, padding: 55 },
      minZoom: 0.35,
      maxZoom: 3
    });
    this.core.on("tap", 'node[kind = "resident"]', (event) => this.kernel.selectViewer(event.target.id()));
  }

  public destroy(): void {
    this.core?.destroy();
    this.core = null;
    this.container?.replaceChildren();
    this.container = null;
  }
}
