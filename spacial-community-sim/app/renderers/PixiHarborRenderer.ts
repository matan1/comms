import { Application, Container, Graphics, Text, TextStyle } from "pixi.js";
import type { SimulationKernel } from "../kernel/SimulationKernel";
import type { HarborGeographyProjection } from "../models/projections";
import type { Point } from "../models/snapshot";
import type { ProjectionRenderer } from "./ProjectionRenderer";

interface ResidentDisplay {
  root: Container;
  marker: Graphics;
  label: Text;
}

export class PixiHarborRenderer implements ProjectionRenderer<HarborGeographyProjection> {
  private readonly app = new Application();
  private readonly background = new Container();
  private readonly residentsLayer = new Container();
  private readonly courierLayer = new Container();
  private readonly residents = new Map<string, ResidentDisplay>();
  private readonly couriers = new Map<string, Container>();
  private mounted = false;
  private backgroundKey = "";

  public constructor(private readonly kernel: SimulationKernel) {}

  public async mount(container: HTMLElement): Promise<void> {
    await this.app.init({
      resizeTo: container,
      preference: "webgl",
      background: "#b8c7a8",
      antialias: true,
      autoDensity: true,
      resolution: Math.min(window.devicePixelRatio || 1, 2)
    });
    container.replaceChildren(this.app.canvas);
    this.app.stage.addChild(this.background, this.residentsLayer, this.courierLayer);
    this.mounted = true;
  }

  public render(projection: HarborGeographyProjection): void {
    if (!this.mounted) return;
    const width = this.app.screen.width;
    const height = this.app.screen.height;
    const key = `${width}:${height}:${projection.settlements.map((settlement) => settlement.id).join(",")}:${projection.viewerId}`;
    if (key !== this.backgroundKey) {
      this.backgroundKey = key;
      this.drawBackground(projection, width, height);
    }
    this.syncResidents(projection, width, height);
    this.syncCouriers(projection, width, height);
  }

  public destroy(): void {
    this.residents.clear();
    this.couriers.clear();
    if (this.mounted) this.app.destroy(true, { children: true });
    this.mounted = false;
  }

  private drawBackground(projection: HarborGeographyProjection, width: number, height: number): void {
    this.background.removeChildren().forEach((child) => child.destroy({ children: true }));
    const water = new Graphics()
      .ellipse(width * 0.50, height * 0.50, width * 0.19, height * 0.17)
      .fill({ color: 0x79a9b4, alpha: 0.92 });
    water.rotation = -0.28;
    water.pivot.set(width * 0.50, height * 0.50);
    water.position.set(width * 0.50, height * 0.50);
    this.background.addChild(water);

    const route = new Graphics();
    const first = projection.route[0];
    if (first) {
      route.moveTo(first.x * width, first.y * height);
      for (const point of projection.route.slice(1)) route.lineTo(point.x * width, point.y * height);
      route.stroke({ color: 0xecf7f2, alpha: 0.78, width: 2 });
    }
    this.background.addChild(route);

    for (const settlement of projection.settlements) {
      const group = new Container({ alpha: settlement.opacity });
      const color = Number.parseInt(settlement.boundaryColor.slice(1), 16);
      group.addChild(new Graphics()
        .circle(settlement.center.x * width, settlement.center.y * height, settlement.radius * width)
        .fill({ color, alpha: 0.11 })
        .stroke({ color, alpha: 0.8, width: 2 }));
      group.addChild(new Text({
        text: settlement.label,
        style: new TextStyle({ fontFamily: "ui-monospace, monospace", fontSize: 14, fontWeight: "bold", fill: 0x243538 }),
        x: (settlement.center.x - 0.09) * width,
        y: (settlement.center.y - 0.16) * height
      }));
      for (const station of settlement.stations) group.addChild(this.station(station.position, station.label, width, height));
      this.background.addChild(group);
    }

    const homes = new Graphics();
    for (const home of projection.homes) {
      const x = home.position.x * width;
      const y = home.position.y * height;
      homes.rect(x - 5, y - 4, 10, 8).fill({ color: home.occupied ? 0xcbb592 : 0xbcae91, alpha: home.opacity });
    }
    this.background.addChild(homes);
  }

  private station(point: Point, label: string, width: number, height: number): Container {
    const root = new Container({ x: point.x * width, y: point.y * height });
    root.addChild(new Graphics().roundRect(-17, -10, 34, 20, 3).fill(0x3f6673));
    const text = new Text({
      text: label,
      style: { fontFamily: "ui-monospace, monospace", fontSize: 7, fontWeight: "bold", fill: 0xf5f0df }
    });
    text.anchor.set(0.5);
    root.addChild(text);
    return root;
  }

  private syncResidents(projection: HarborGeographyProjection, width: number, height: number): void {
    const live = new Set(projection.residents.map((resident) => resident.id));
    for (const [id, display] of this.residents) {
      if (live.has(id)) continue;
      display.root.destroy({ children: true });
      this.residents.delete(id);
    }
    for (const resident of projection.residents) {
      let display = this.residents.get(resident.id);
      if (!display) {
        const root = new Container({ eventMode: "static", cursor: "pointer" });
        const marker = new Graphics();
        const label = new Text({
          text: resident.label,
          style: { fontFamily: "ui-monospace, monospace", fontSize: 10, fontWeight: "bold", fill: 0x243538 }
        });
        label.x = 12;
        label.y = -6;
        label.visible = false;
        root.addChild(marker, label);
        root.on("pointertap", () => this.kernel.selectViewer(resident.id));
        root.on("pointerover", () => { label.visible = true; });
        root.on("pointerout", () => { label.visible = resident.selected; });
        this.residentsLayer.addChild(root);
        display = { root, marker, label };
        this.residents.set(resident.id, display);
      }
      display.root.position.set(resident.position.x * width, resident.position.y * height);
      display.root.alpha = resident.opacity;
      display.label.visible = resident.selected;
      display.marker.clear()
        .circle(0, 0, resident.radius)
        .fill(resident.fill)
        .stroke({ color: resident.selected ? 0x1e2527 : 0x384441, width: resident.selected ? 3 : 1.4, alpha: 0.72 });
    }
  }

  private syncCouriers(projection: HarborGeographyProjection, width: number, height: number): void {
    for (const courier of projection.couriers) {
      let root = this.couriers.get(courier.id);
      if (!root) {
        root = new Container();
        root.addChild(new Graphics()
          .poly([0, -8, 9, 6, -8, 4])
          .fill(0xf2ead1)
          .stroke({ color: 0x315864, width: 1.5 }));
        const label = new Text({
          text: courier.label,
          style: { fontFamily: "ui-monospace, monospace", fontSize: 10, fill: 0x243538 },
          x: 12,
          y: -5
        });
        label.label = "courier-label";
        root.addChild(label);
        this.courierLayer.addChild(root);
        this.couriers.set(courier.id, root);
      }
      root.position.set(courier.position.x * width, courier.position.y * height);
      const label = root.getChildByLabel("courier-label") as Text | null;
      if (label) label.text = `${courier.label} · ${courier.status}`;
    }
  }
}
