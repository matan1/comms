export interface ProjectionRenderer<TProjection> {
  mount(container: HTMLElement): Promise<void> | void;
  render(projection: TProjection): void;
  destroy(): void;
}

