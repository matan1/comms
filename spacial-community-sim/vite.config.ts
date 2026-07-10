import { defineConfig } from "vitest/config";
import { copyFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";

const legacyAssets = [
  "world.js", "sim.js", "harbor.js", "render.js", "ui.js", "legacy-bridge.js"
];

export default defineConfig({
  server: { host: "127.0.0.1" },
  build: { target: "es2022" },
  test: { environment: "node", include: ["tests/**/*.test.ts"] },
  plugins: [{
    name: "copy-legacy-kernel",
    async writeBundle(options) {
      const output = typeof options.dir === "string" ? options.dir : "dist";
      await mkdir(output, { recursive: true });
      await Promise.all(legacyAssets.map((asset) => copyFile(resolve(asset), resolve(output, asset))));
    }
  }]
});
