import type { LegacyHarborSnapshot } from "../models/snapshot";

export interface SimulationView {
  worldKind: string | null;
  projectionMode: string | null;
}

export interface SimulationKernel {
  getSnapshot(): LegacyHarborSnapshot | null;
  getView(): SimulationView;
  selectViewer(id: string | null): void;
  subscribe(listener: EventListener): () => void;
}

declare global {
  interface Window {
    CommsSimulation?: SimulationKernel;
  }
}

export function legacyKernel(): SimulationKernel {
  const kernel = window.CommsSimulation;
  if (!kernel) throw new Error("The legacy simulation bridge is unavailable.");
  return kernel;
}

