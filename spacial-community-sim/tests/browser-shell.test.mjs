import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import test from "node:test";
import { chromium } from "playwright";

const baseUrl = process.env.COMMS_SIM_URL || "http://127.0.0.1:4173";

async function previewServer() {
  if (process.env.COMMS_SIM_URL) return null;
  const vite = new URL("../node_modules/vite/bin/vite.js", import.meta.url);
  const server = spawn(process.execPath, [vite.pathname, "preview", "--host", "127.0.0.1", "--port", "4173"], {
    stdio: "ignore"
  });
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch(baseUrl);
      if (response.ok) return server;
    } catch {
      // Preview is still starting.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  server.kill();
  throw new Error("Vite preview did not start.");
}

test("shell selects Pixi geography, Cytoscape evidence, and legacy fallback", async () => {
  const server = await previewServer();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  try {
    await page.goto(baseUrl, { waitUntil: "networkidle" });
    await page.selectOption("#worldMode", "harbor");
    await page.waitForSelector("#mapStage.modern-renderer-active #modernStage:not([hidden]) canvas");
    assert.equal(await page.locator("#surveyTitle").textContent(), "Archive Harbor");
    assert.equal(await page.locator("#harborProjection").inputValue(), "geography");
    assert.equal(await page.locator("#modernStage").getAttribute("data-renderer"), "pixi");
    assert.equal(await page.locator("#endpointControl").evaluate((element) => getComputedStyle(element).display), "none");

    await page.evaluate(() => {
      const snapshot = window.CommsSimulation?.getSnapshot();
      window.CommsSimulation?.selectViewer(snapshot?.residents[0]?.id ?? null);
    });
    await page.waitForFunction(() => window.CommsSimulation?.getSnapshot()?.viewer !== null);
    assert.match(await page.locator("#viewpointPill").textContent() ?? "", /^Seen by /);

    await page.selectOption("#harborProjection", "evidence");
    await page.waitForFunction(() => document.querySelectorAll("#modernStage canvas").length >= 2);
    assert.equal(await page.locator("#modernStage").getAttribute("data-renderer"), "cytoscape");
    assert.equal(await page.locator("#mapStage").evaluate((element) => element.classList.contains("modern-renderer-active")), true);

    await page.selectOption("#harborProjection", "custody");
    await page.waitForFunction(() => document.getElementById("modernStage")?.hidden === true);
    assert.equal(await page.locator("#mapStage").evaluate((element) => element.classList.contains("modern-renderer-active")), false);
    assert.equal(await page.locator("#modernStage").getAttribute("data-renderer"), "legacy");
    assert.equal(errors.length, 0, errors.join("\n"));
  } finally {
    await browser.close();
    server?.kill();
  }
});
