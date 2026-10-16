// Real-engine browser checks for URL presentation and the iframe contract.
const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const site = path.resolve(process.env.SITE_DIR || "docs/book/src/explore");
const out = process.env.OUT_DIR || fs.mkdtempSync(path.join(os.tmpdir(), "explore-presentation-"));
fs.mkdirSync(out, { recursive: true });
const mime = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://localhost");
  if (url.pathname === "/host") {
    res.setHeader("content-type", "text/html");
    res.end('<body style="margin:0"><iframe title="Playground" style="width:100%;height:860px;border:0" src="/?embed=1&theme=intigraph"></iframe><script>window.heights=[];addEventListener("message",e=>{const f=document.querySelector("iframe");if(e.origin===location.origin&&e.source===f.contentWindow&&e.data.type==="quipu:resize"){window.heights.push(e.data.height);f.style.height=e.data.height+"px";}});</script>');
    return;
  }
  try {
    const name = decodeURIComponent(url.pathname.endsWith("/") ? url.pathname + "index.html" : url.pathname);
    const file = path.resolve(site, "." + name);
    if (!file.startsWith(site + path.sep)) throw Error("Outside site");
    res.setHeader("content-type", mime[path.extname(file)] || "application/octet-stream");
    res.end(fs.readFileSync(file));
  } catch { res.writeHead(404); res.end("Not found"); }
});
let browser;
(async () => {
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const target = process.env.TARGET_URL || `http://127.0.0.1:${server.address().port}/`;
  browser = await chromium.launch({ headless: false,
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const context = await browser.newContext({ viewport: { width: 1200, height: 860 }, colorScheme: "dark",
    recordVideo: { dir: out, size: { width: 1200, height: 860 } } });
  const page = await context.newPage();
  const errors = [], checks = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const check = (name, ok, detail) => {
    checks.push({ name, ok: !!ok, detail });
    console.log(`${ok ? "PASS" : "FAIL"} ${name}`);
    assert(ok, name + ": " + JSON.stringify(detail));
  };
  const ready = (p) => p.waitForFunction(() => !document.querySelector("main").hidden
    && document.querySelector("#graph-targets").children.length > 0, null, { timeout: 120000 });
  try {
    await page.goto(target);
    await ready(page);
    check("normal page keeps Ask and chrome", await page.locator("#tab-ask").getAttribute("aria-selected") === "true"
      && await page.locator(".top").isVisible() && await page.locator(".hero").isVisible());
    await page.selectOption("#theme-choice", "intigraph");
    check("picker applies Intigraph dark tokens", await page.evaluate(() => getComputedStyle(document.body).backgroundColor) === "rgb(18, 18, 31)");
    await page.selectOption("#theme-choice", "light");
    check("existing Light choice still applies", await page.evaluate(() => getComputedStyle(document.body).backgroundColor) === "rgb(238, 240, 243)");
    const samples = [], fullReadySamples = [];
    for (let i = 0; i < 2; i++) {
      const start = Date.now();
      await page.goto(new URL("?embed=1&theme=intigraph", target).href);
      await ready(page);
      samples.push(Date.now() - start);
      console.log("ENGINE", JSON.stringify(await page.evaluate(() => window.__quipuBuild)));
      if (i === 1) {
        check("warm Map is labelled preview during verification", await page.locator("html").getAttribute("data-map-preview") === "true"
          && (await page.locator("#status").innerText()).includes("Saved map preview"));
        check("preview keeps queries and writes disabled", await page.locator("#tab-ask").isDisabled()
          && await page.locator("#bad-write").isDisabled() && await page.locator("#run").isDisabled());
      }
      await page.waitForFunction(() => !document.documentElement.dataset.mapPreview
        && document.querySelector('#node-card[aria-busy="false"]'), null, { timeout: 120000 });
      fullReadySamples.push(Date.now() - start);
    }
    check("embed URL opens real populated Map", await page.locator("#tab-map").getAttribute("aria-selected") === "true"
      && await page.locator("#graph-targets button").count() > 0);
    check("embed hides navigation, loader, hero and take-away", !(await page.locator(".top").isVisible())
      && !(await page.locator("#load-menu").isVisible()) && !(await page.locator(".hero").isVisible())
      && !(await page.locator(".take").isVisible()));
    check("desktop content fits 860px", await page.evaluate(() => document.body.getBoundingClientRect().height <= 860));
    await page.screenshot({ path: path.join(out, "embed-dark.png"), fullPage: true });
    await page.click("#tab-ask");
    await page.waitForSelector("#sparql-out tbody tr");
    check("Ask has a real positive answer", await page.locator("#sparql-out tbody tr").count() > 0);
    await page.click("#bad-write");
    await page.waitForFunction(() => document.querySelector("#bad-verdict .proof"), null, { timeout: 30000 });
    check("bad write has refusal and populated control", (await page.locator("#bad-verdict").innerText()).includes("Existing graph control: 1 row"));
    await page.click("#tab-map");
    await page.emulateMedia({ colorScheme: "light" });
    check("Intigraph light variant", await page.evaluate(() => getComputedStyle(document.body).backgroundColor) === "rgb(251, 250, 246)");
    await page.setViewportSize({ width: 390, height: 860 });
    check("390px has no horizontal overflow", await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    check("mobile graph precedes starter questions", await page.locator(".center").evaluate((e) => e.getBoundingClientRect().top < document.querySelector(".starters").getBoundingClientRect().top));
    await page.screenshot({ path: path.join(out, "embed-mobile.png"), fullPage: true });
    if (!process.env.TARGET_URL) {
      await page.setViewportSize({ width: 1200, height: 860 });
      await page.goto(new URL("host", target).href);
      const frame = page.frameLocator("iframe");
      await frame.locator("#graph-targets button").first().waitFor({ timeout: 120000 });
      await page.waitForFunction(() => window.heights.some((h) => h > 400));
      check("iframe receives height messages", await page.evaluate(() => window.heights.every((h) => Number.isInteger(h) && h > 0)));
      await frame.locator('#node-card[aria-busy="false"]').waitFor();
      const mapHeight = await page.evaluate(() => window.heights.at(-1));
      await frame.locator("#tab-ask").click();
      await page.waitForFunction((old) => Math.abs(window.heights.at(-1) - old) > 1, mapHeight);
      const askHeight = await page.evaluate(() => window.heights.at(-1));
      await frame.locator("#tab-map").click();
      await page.waitForFunction((old) => Math.abs(window.heights.at(-1) - old) > 1, askHeight);
      check("parent can resize both directions", await page.evaluate((expected) =>
        Math.abs(window.heights.at(-1) - expected) <= 2, mapHeight), { mapHeight, askHeight });
    }
    const runtime = page.url().endsWith("/host") ? page.frames().find((frame) => frame !== page.mainFrame()) : page;
    const cacheChecks = await runtime.evaluate(async () => {
      const { mapCache } = await import("./map-cache.js");
      const slot = "quipu:map-preview:v1", original = localStorage.getItem(slot);
      const bytes = await (await fetch("./repository.pendant.tar.gz")).arrayBuffer();
      const build = window.__quipuBuild, cache = await mapCache(bytes, build);
      const snapshot = cache.read(), results = {};
      try {
        results.populated = snapshot.knowledge.rows.length > 0 && snapshot.files.rows.length > 0;
        const changed = bytes.slice(0); new Uint8Array(changed)[0] ^= 1;
        results.changedBytesMiss = (await mapCache(changed, build)).read() === null;
        results.changedEngineMiss = (await mapCache(bytes, { ...build, git_sha: "different" })).read() === null;
        results.changedFeaturesMiss = (await mapCache(bytes, { ...build, compiled_features: {} })).read() === null;
        localStorage.setItem(slot, "broken json");
        results.corruptMiss = cache.read() === null;
        localStorage.setItem(slot, JSON.stringify({ ...JSON.parse(original), snapshot: { knowledge: { rows: [null] } } }));
        results.badRowsMiss = cache.read() === null;
        const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
        try {
          Object.defineProperty(window, "localStorage", { configurable: true, get() { throw new Error("storage denied"); } });
          results.storageDeniedMiss = await mapCache(bytes, build) === null;
        } finally { Object.defineProperty(window, "localStorage", descriptor); }
      } finally { localStorage.setItem(slot, original); }
      return results;
    });
    for (const [name, ok] of Object.entries(cacheChecks)) check("cache boundary " + name, ok);
    // A synthetic worker refusal proves a saved picture cannot stand in for
    // successful native verification. The earlier positive controls use WASM.
    const savedPreview = await page.evaluate(() => localStorage.getItem("quipu:map-preview:v1"));
    await page.addInitScript((seed) => {
      if (location.search.includes("injected-refusal")) localStorage.setItem("quipu:map-preview:v1", seed);
      const NativeWorker = Worker;
      window.Worker = class extends NativeWorker {
        postMessage(message, ...rest) {
          if (message.cmd !== "load") return super.postMessage(message, ...rest);
          setTimeout(() => this.dispatchEvent(new MessageEvent("message", { data: {
            id: message.id, ...(location.search.includes("refused-report")
              ? { ok: true, result: { promotion: null } }
              : { ok: false, error: "Injected verification refusal" }),
          } })), 300);
        }
      };
    }, savedPreview);
    for (const [mode, message] of [["worker-error", "Injected verification refusal"], ["refused-report", "fresh pack was not promoted"]]) {
      await page.goto(new URL(`?embed=1&theme=intigraph&injected-refusal=${mode}`, target).href);
      await page.waitForFunction((text) => document.querySelector("main").hidden
        && document.querySelector("#status").textContent.includes(text), message);
      check(`${mode} removes saved preview`, await page.evaluate(() => localStorage.getItem("quipu:map-preview:v1") === null));
    }
    check("no browser errors", errors.length === 0, errors);
    console.log(JSON.stringify({ samples, fullReadySamples, warmUnderFiveSeconds: samples[1] < 5000 }));
    // Keep performance explicit; presentation correctness must not turn a slow
    // released engine into a fictitious pass.
    fs.writeFileSync(path.join(out, "results.json"), JSON.stringify({ checks, samples, fullReadySamples, errors }, null, 2) + "\n");
    if (process.argv.includes("--require-speed")) check("warm graph under five seconds", samples[1] < 5000, samples);
  } catch (error) {
    await page.screenshot({ path: path.join(out, "failure.png"), fullPage: true });
    fs.writeFileSync(path.join(out, "failure.json"), JSON.stringify({ error: error.message, errors,
      status: await page.locator("#status").textContent().catch(() => "host"), checks }, null, 2) + "\n");
    throw error;
  } finally { await context.close(); }
})().catch((e) => { console.error(e); process.exitCode = 1; }).finally(async () => {
  if (browser) await browser.close();
  server.close();
});
