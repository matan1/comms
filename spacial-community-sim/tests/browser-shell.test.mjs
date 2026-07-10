import assert from "node:assert/strict";
import test from "node:test";
import { chromium } from "playwright";

const baseUrl = process.env.COMMS_SIM_URL || "http://127.0.0.1:4173";

test("shell selects Pixi geography, Cytoscape evidence, and legacy fallback", async () => {
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
  }
});
