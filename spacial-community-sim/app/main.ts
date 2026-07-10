import { legacyKernel } from "./kernel/SimulationKernel";
import { RendererShell } from "./shell/RendererShell";

const stage = document.getElementById("mapStage");
const host = document.getElementById("modernStage");

if (stage && host) {
  const shell = new RendererShell(legacyKernel(), stage, host);
  void shell.start();
  window.addEventListener("beforeunload", () => shell.destroy(), { once: true });
}

