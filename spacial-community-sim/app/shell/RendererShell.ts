import type { SimulationKernel } from "../kernel/SimulationKernel";
import { projectEvidenceGraph } from "../projections/evidenceGraph";
import { projectHarborGeography } from "../projections/harborGeography";
import type { CytoscapeEvidenceRenderer } from "../renderers/CytoscapeEvidenceRenderer";
import type { PixiHarborRenderer } from "../renderers/PixiHarborRenderer";

type ActiveRenderer = "legacy" | "pixi" | "cytoscape";

export class RendererShell {
  private active: ActiveRenderer = "legacy";
  private pixi: PixiHarborRenderer | null = null;
  private cytoscape: CytoscapeEvidenceRenderer | null = null;
  private unsubscribe: (() => void) | null = null;
  private frame = 0;
  private activation = 0;

  public constructor(
    private readonly kernel: SimulationKernel,
    private readonly stage: HTMLElement,
    private readonly host: HTMLElement
  ) {}

  public async start(): Promise<void> {
    this.unsubscribe = this.kernel.subscribe(() => { void this.syncRenderer(); });
    await this.syncRenderer();
    this.tick();
  }

  public destroy(): void {
    cancelAnimationFrame(this.frame);
    this.unsubscribe?.();
    this.destroyModernRenderer();
  }

  private desiredRenderer(): ActiveRenderer {
    const view = this.kernel.getView();
    if (view.worldKind !== "harbor") return "legacy";
    if (view.projectionMode === "geography") return "pixi";
    if (view.projectionMode === "evidence") return "cytoscape";
    return "legacy";
  }

  private async syncRenderer(): Promise<void> {
    const desired = this.desiredRenderer();
    if (desired === this.active) {
      if (desired === "cytoscape") this.renderCytoscape();
      return;
    }
    const activation = ++this.activation;
    this.destroyModernRenderer();
    this.active = desired;
    this.stage.classList.toggle("modern-renderer-active", desired !== "legacy");
    this.host.hidden = desired === "legacy";
    this.host.dataset.renderer = desired;
    if (desired === "pixi") {
      const { PixiHarborRenderer } = await import("../renderers/PixiHarborRenderer");
      if (activation !== this.activation) return;
      const renderer = new PixiHarborRenderer(this.kernel);
      await renderer.mount(this.host);
      if (activation !== this.activation) {
        renderer.destroy();
        return;
      }
      this.pixi = renderer;
    } else if (desired === "cytoscape") {
      const { CytoscapeEvidenceRenderer } = await import("../renderers/CytoscapeEvidenceRenderer");
      if (activation !== this.activation) return;
      this.cytoscape = new CytoscapeEvidenceRenderer(this.kernel);
      this.cytoscape.mount(this.host);
      this.renderCytoscape();
    }
  }

  private tick = (): void => {
    if (this.active === "pixi" && this.pixi) {
      const snapshot = this.kernel.getSnapshot();
      if (snapshot) this.pixi.render(projectHarborGeography(snapshot));
    }
    this.frame = requestAnimationFrame(this.tick);
  };

  private renderCytoscape(): void {
    const snapshot = this.kernel.getSnapshot();
    if (snapshot && this.cytoscape) this.cytoscape.render(projectEvidenceGraph(snapshot));
  }

  private destroyModernRenderer(): void {
    this.pixi?.destroy();
    this.cytoscape?.destroy();
    this.pixi = null;
    this.cytoscape = null;
    this.host.replaceChildren();
  }
}
