// Real worker + release pendant, with slow/failing HTTP delivery at the edge.
// No worker, import result, percentage or successful query is fabricated.
const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const { gzipSync } = require("node:zlib");
const site = path.resolve(process.env.SITE_DIR || "docs/book/src/explore");
const out = process.env.OUT_DIR || fs.mkdtempSync(path.join(os.tmpdir(), "explore-loading-"));
const baseline = process.env.BASELINE === "1";
fs.mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let mode = {}, downloads = 0, wasmRequests = 0;
const mime = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript",
  ".css": "text/css", ".wasm": "application/wasm", ".gz": "application/gzip" };
const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://localhost");
    let name = decodeURIComponent(url.pathname);
    if (name.endsWith("/")) name += "index.html";
    if (name === "/selected.pendant.tar.gz") name = "/repository.pendant.tar.gz";
    const file = path.resolve(site, "." + name);
    if (!file.startsWith(site + path.sep)) throw Error("Outside site");
    if (name.endsWith(".wasm")) {
      wasmRequests++;
      await sleep(mode.engineDelay || 0);
      if (mode.failEngine) { mode.failEngine--; res.writeHead(503); res.end("Unavailable"); return; }
    }
    const pack = name.endsWith(".pendant.tar.gz");
    if (pack) {
      downloads++;
      if (mode.failPack) { mode.failPack--; res.writeHead(503); res.end("Unavailable"); return; }
    }
    const data = pack && mode.damaged ? Buffer.from("damaged pendant") : fs.readFileSync(file);
    res.setHeader("content-type", mime[path.extname(file)] || "application/octet-stream");
    res.setHeader("cache-control", "no-store");
    if (!(pack && mode.unknownLength)) res.setHeader("content-length", data.length);
    if (req.method === "HEAD") { res.end(); return; }
    if (pack && mode.slow) {
      for (let offset = 0; offset < data.length && !res.destroyed; offset += 131072) {
        res.write(data.subarray(offset, offset + 131072));
        await sleep(100);
      }
      res.end();
    } else res.end(data);
  } catch { res.writeHead(404); res.end("Not found"); }
});
// A second origin hides Content-Encoding through the real browser CORS filter.
const corsServer = http.createServer(async (_req, res) => {
  const data = gzipSync(fs.readFileSync(path.join(site, "repository.pendant.tar.gz")));
  res.writeHead(200, { "access-control-allow-origin": "*", "content-type": "application/gzip",
    "content-encoding": "gzip", "content-length": data.length, "cache-control": "no-store" });
  for (let i = 0; i < data.length && !res.destroyed; i += 131072) {
    res.write(data.subarray(i, i + 131072));
    await sleep(100);
  }
  res.end();
});

// CI has reported a capture protocol error before its first screenshot.
// Retry only that protocol error; state assertions and a persistent failure remain fatal.
async function screenshot(page, options) {
  for (let attempt = 0; ; attempt++) {
    try { return await page.screenshot(options); }
    catch (error) {
      if (attempt === 2 || !error.message.includes("Unable to capture screenshot")) throw error;
      await page.waitForTimeout(100);
    }
  }
}

let browser;
const results = [];
function check(name, value) { assert(value, name); results.push(name); console.log("PASS", name); }
(async () => {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  await new Promise((resolve) => corsServer.listen(0, "127.0.0.1", resolve));
  const target = process.env.TARGET_URL || `http://127.0.0.1:${server.address().port}/`;
  browser = await chromium.launch({ headless: process.env.CI === "true",
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function open({ video = false, mobile = false } = {}) {
    const context = await browser.newContext({ viewport: mobile
      ? { width: 375, height: 812 } : { width: 1200, height: 900 }, colorScheme: "dark",
    ...(video ? { recordVideo: { dir: out, size: { width: 1200, height: 900 } } } : {}) });
    const page = await context.newPage();
    await page.addInitScript(() => {
      window.loadStates = [];
      new MutationObserver(() => {
        const state = document.querySelector("#graph-loader")?.dataset.state;
        if (state && window.loadStates.at(-1) !== state) window.loadStates.push(state);
      }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ["data-state"] });
    });
    return { page, context };
  }
  async function ready(page) {
    await page.waitForFunction(() => !document.querySelector("main").hidden
      && document.querySelector("#graph-targets").children.length > 0, null, { timeout: 120000 });
    if (!baseline) await page.waitForSelector('#graph-loader[data-state="ready"]', { state: "attached", timeout: 120000 });
    const result = await page.evaluate(() => window.quipu.query("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1"));
    check("ready workbench answers a real positive query", result.rows.length === 1);
  }
  try {
    mode = { engineDelay: 1600, slow: true };
    let { page, context } = await open({ video: true });
    await page.goto(target, { waitUntil: "domcontentloaded" });
    if (baseline) {
      check("before: workbench hidden during engine startup", await page.locator("main").getAttribute("hidden") !== null);
      check("before: no canvas loading panel", await page.locator("#graph-loader").count() === 0);
      await screenshot(page, { path: path.join(out, "before.png"), fullPage: true });
      await ready(page);
    } else {
      await page.waitForSelector('#graph-loader[data-state="engine"]');
      check("engine has a prominent loading panel", (await page.locator("#graph-loader").boundingBox()).height >= 300);
      await screenshot(page, { path: path.join(out, "engine.png"), fullPage: true });
      await page.waitForFunction(() => {
        const p = document.querySelector("#loading-progress");
        return document.querySelector("#graph-loader").dataset.state === "pendant" && p.value > 0 && p.value < 100;
      });
      check("pendant percentage reflects an in-flight byte stream", (await page.locator("#loading-bytes").innerText()).includes("%"));
      await screenshot(page, { path: path.join(out, "pendant.png"), fullPage: true });
      await page.waitForSelector('#graph-loader[data-state="indexing"]');
      check("import and indexing stay visibly in progress", await page.locator("#graph-loader").getAttribute("aria-busy") === "true");
      await screenshot(page, { path: path.join(out, "indexing.png"), fullPage: true });
      await ready(page);
      check("observed all actual loading stages in order", JSON.stringify(await page.evaluate(() => window.loadStates))
        === JSON.stringify(["engine", "pendant", "indexing", "ready"]));
      check("ready dismisses the loading panel", !(await page.locator("#graph-loader").isVisible()));
      await screenshot(page, { path: path.join(out, "ready.png"), fullPage: true });
      fs.writeFileSync(path.join(out, "engine.json"), JSON.stringify(await page.evaluate(() => window.__quipuBuild), null, 2));
    }
    const video = page.video();
    await context.close();
    if (video) fs.renameSync(await video.path(), path.join(out, baseline ? "before.webm" : "after.webm"));
    if (baseline) return;

    mode = { slow: true, unknownLength: true };
    ({ page, context } = await open({ mobile: true }));
    await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "light" });
    await page.goto(target);
    await page.waitForFunction(() => document.querySelector("#graph-loader").dataset.state === "pendant"
      && !document.querySelector("#loading-bytes").textContent.startsWith("0 B"));
    check("unknown size stays indeterminate without an invented percentage", await page.locator("#loading-progress").getAttribute("value") === null
      && !(await page.locator("#loading-bytes").innerText()).includes("%"));
    check("mobile loader fits the viewport", await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    const mobilePanel = await page.locator("#graph-loader").boundingBox();
    check("mobile loading stages stay above the fold", mobilePanel.y + mobilePanel.height <= page.viewportSize().height);
    check("reduced motion removes pulsing", await page.locator(".loading-mark").evaluate((e) => getComputedStyle(e).animationName) === "none");
    await screenshot(page, { path: path.join(out, "mobile.png"), fullPage: true });
    await ready(page);
    await context.close();

    for (const kind of ["failPack", "failEngine", "damaged"]) {
      mode = { [kind]: 1 };
      const startDownloads = downloads, startEngine = wasmRequests;
      ({ page, context } = await open());
      await page.goto(target);
      await page.waitForSelector('#graph-loader[data-state="error"]', { timeout: 120000 });
      check(`${kind}: visible error and retry, no false ready`, await page.locator("#loading-retry").isVisible()
        && !(await page.locator("main").isVisible())
        && !(await page.evaluate(() => window.loadStates)).includes("ready"));
      await screenshot(page, { path: path.join(out, `${kind}.png`), fullPage: true });
      mode = {};
      await page.click("#loading-retry");
      await ready(page);
      check(`${kind}: retry performs fresh work`, kind === "failEngine" ? wasmRequests > startEngine + 1 : downloads > startDownloads + 1);
      await context.close();
    }

    mode = {};
    ({ page, context } = await open());
    await page.goto(target);
    await ready(page);
    const starters = await page.locator("#starter-questions button").count();
    mode = { failPack: 1 };
    await page.locator("#load-menu").evaluate((e) => { e.open = true; });
    await page.fill("#url-input", new URL("selected.pendant.tar.gz", target).href);
    await page.click("#url-load");
    await page.waitForSelector('#graph-loader[data-state="error"]');
    check("selected URL is retained for its retry", (await page.locator("#status").innerText()).includes("selected.pendant.tar.gz"));
    mode = {};
    await page.click("#loading-retry");
    await ready(page);
    check("retries do not duplicate controls", await page.locator("#starter-questions button").count() === starters);
    await page.fill("#url-input", `http://127.0.0.1:${corsServer.address().port}/compressed.pendant.tar.gz`);
    await page.click("#url-load");
    await page.waitForFunction(() => document.querySelector("#graph-loader").dataset.state === "pendant"
      && !document.querySelector("#loading-bytes").textContent.startsWith("0 B"));
    check("CORS-hidden encoding cannot invent a percentage", await page.locator("#loading-progress").getAttribute("value") === null
      && !(await page.locator("#loading-bytes").innerText()).includes("%"));
    await ready(page);
    await context.close();
  } finally {
    fs.writeFileSync(path.join(out, "results.json"), JSON.stringify({ baseline, results }, null, 2));
  }
})().catch((error) => { console.error(error); process.exitCode = 1; }).finally(async () => {
  await browser?.close();
  server.close();
  corsServer.close();
});
