// Real-browser acceptance of Map navigation against the loaded WASM graph.
// Stage a bundle/pack first with `just explorer`; optionally pass --headed.
const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");

const site = path.resolve(process.env.SITE_DIR || "docs/book/src/explore");
const retained = process.env.OUT_DIR;
const out = retained || fs.mkdtempSync(path.join(os.tmpdir(), "explore-navigation-"));
let browser;
fs.mkdirSync(out, { recursive: true });
const mime = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css",
  ".wasm": "application/wasm", ".gz": "application/gzip" };
const server = http.createServer((req, res) => {
  try {
    let name = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
    if (name.endsWith("/")) name += "index.html";
    const file = path.resolve(site, "." + name);
    if (!file.startsWith(site + path.sep)) throw Error("Outside site");
    res.setHeader("content-type", mime[path.extname(file)] || "application/octet-stream");
    res.end(fs.readFileSync(file));
  } catch { res.writeHead(404); res.end("Not found"); }
});

(async () => {
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const target = `http://127.0.0.1:${server.address().port}/`;
  browser = await chromium.launch({
    headless: !process.argv.includes("--headed"),
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}),
  });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1050 },
    recordVideo: { dir: out, size: { width: 1440, height: 1050 } } });
  const page = await context.newPage(), checks = [], errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const check = (name, value, detail) => {
    checks.push({ name, ok: !!value, detail });
    console.log(`${value ? "PASS" : "FAIL"} ${name}`);
    assert(value, name + ": " + JSON.stringify(detail));
  };
  const settle = async (id) => {
    await page.waitForFunction((wanted) => {
      const card = document.querySelector("#node-card");
      return card.dataset.entity === wanted && card.getAttribute("aria-busy") === "false"
        && card.querySelector(".entity-types");
    }, id, { timeout: 30000 });
  };
  // A reload imports the real pack again; use the initial startup budget,
  // then keep the shorter entity-navigation checks below unchanged.
  const initialAnswer = () => page.waitForFunction(() => document.querySelector("#sparql-out tbody tr"),
    null, { timeout: 120000 });
  const current = () => page.locator("#node-card").getAttribute("data-entity");
  const verify = async (id) => {
    await settle(id);
    const expected = await page.evaluate(async (iri) => {
      // Independent queries: do not import the UI's relation extraction.
      const a = await window.quipu.query(`SELECT ?p ?v WHERE {
        <${iri}> ?predicate ?object . FILTER(isIRI(?object))
        BIND(STR(?predicate) AS ?p) BIND(STR(?object) AS ?v) }`);
      const b = await window.quipu.query(`SELECT ?p ?v WHERE {
        ?subject ?predicate <${iri}> .
        BIND(STR(?predicate) AS ?p) BIND(STR(?subject) AS ?v) }`);
      if (a.truncated || b.truncated) throw Error("Acceptance query was truncated");
      return [...new Set([...a.rows.map((r) => JSON.stringify(["outgoing", r.p, r.v])),
        ...b.rows.map((r) => JSON.stringify(["incoming", r.p, r.v]))])].sort();
    }, id);
    const actual = await page.locator("#node-card .node-links button").evaluateAll((buttons) =>
      buttons.map((b) => JSON.stringify([b.dataset.direction, b.dataset.predicate, b.dataset.entity])).sort());
    check("related links equal independent SPARQL", JSON.stringify(actual) === JSON.stringify(expected), { id, links: actual.length });
    check("type, property and provenance disclosures", await page.locator("#node-card .entity-properties").count() === 1
      && await page.locator("#node-card .entity-provenance").count() === 1);
    const centered = await page.locator("#graph-targets .node-target").evaluateAll((buttons, wanted) => {
      const b = buttons.find((v) => v.dataset.node === wanted);
      const svg = document.querySelector("#graph");
      return b && Math.abs(parseFloat(b.style.left) + 22 - svg.clientWidth / 2) < 1
        && Math.abs(parseFloat(b.style.top) + 22 - svg.clientHeight / 2) < 1;
    }, id);
    check("selected node is at map center", centered, id);
    check("URL identifies selected entity", new URL(page.url()).searchParams.get("entity") === id);
    return actual.length;
  };
  try {
    await page.goto(target);
    await initialAnswer();
    check("first answer positive control", await page.locator("#sparql-out tbody tr").count() > 0);
    await page.click("#tab-map");
    await page.locator("#graph-targets .node-target").first().click();
    await page.waitForFunction(() => document.querySelector("#node-card").getAttribute("aria-busy") === "false");
    const trail = [await current()];
    await verify(trail[0]);
    for (let hop = 0; hop < 3; hop++) {
      const links = page.locator("#node-card .node-links button");
      const candidates = await links.evaluateAll((bs) => bs.map((b) => b.dataset.entity));
      const index = candidates.findIndex((id) => !trail.includes(id));
      check("unvisited clickable neighbour", index >= 0, { hop, candidates: candidates.length });
      const next = candidates[index];
      await links.nth(index).click(); await verify(next); trail.push(next);
    }
    for (const id of trail.slice(0, -1).reverse()) { await page.goBack(); await verify(id); }
    for (const id of trail.slice(1)) { await page.goForward(); await verify(id); }
    const saved = page.url(); await page.reload(); await initialAnswer(); await verify(trail.at(-1));
    check("reload restores Map tab", await page.locator("#tab-map").getAttribute("aria-selected") === "true");
    check("deep link survives reload", saved === page.url());
    // A valid IRI not in the overview can still be opened through an incoming link.
    const type = await page.evaluate(async () => {
      // Use a low-degree real type so the independent full-link oracle below
      // remains complete as the public repository pendant grows.
      const r = await window.quipu.query("SELECT ?iri (COUNT(?s) AS ?members) WHERE { ?s a ?type . BIND(STR(?type) AS ?iri) } GROUP BY ?iri ORDER BY ?members LIMIT 1");
      return r.rows[0].iri;
    });
    const external = new URL(page.url()); external.searchParams.set("entity", type);
    await page.goto(external.href); await initialAnswer(); await verify(type);
    check("outside-overview entity has incoming links", await page.locator('#node-card button[data-direction="incoming"]').count() > 0);
    // Delay a real worker query, then navigate back while it is in flight.
    const delayed = await page.locator("#node-card .node-links button").first().getAttribute("data-entity");
    await page.evaluate((id) => {
      const send = Worker.prototype.postMessage;
      window.delayedQueries = 0;
      Worker.prototype.postMessage = function (message, ...rest) {
        if (message.cmd === "query" && message.sparql.includes(`<${id}>`)) {
          window.delayedQueries++;
          setTimeout(() => { send.call(this, message, ...rest); window.delayedQueries--; }, 700);
        } else send.call(this, message, ...rest);
      };
      window.restoreWorker = () => { Worker.prototype.postMessage = send; };
    }, delayed);
    await page.locator("#node-card .node-links button").first().click();
    await page.goBack(); await settle(type);
    await page.waitForFunction(() => window.delayedQueries === 0);
    await page.evaluate(() => window.restoreWorker());
    await verify(type);
    check("late response cannot overwrite back navigation", await current() === type);
    await page.locator("#node-card > button").click();
    check("fact edit drawer retained", await page.locator("#facts-drawer").isVisible());
    await page.click("#facts-close");
    await page.locator("#node-card .entity-properties").evaluate((e) => { e.open = true; });
    await page.locator("#node-card .entity-provenance").evaluate((e) => { e.open = true; });
    await page.screenshot({ path: path.join(out, "map-desktop.png") });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(out, "map-mobile.png"), fullPage: true });
    check("mobile has no horizontal overflow", await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    // A second real pack without the vision must not let the file browser
    // overwrite the restored Map selection. All writes remain in this tab.
    const pack = await page.evaluate(async () => {
      const vision = await window.quipu.query("SELECT (STR(?x) AS ?v) WHERE { ?x a <https://quipu.dev/knowledge/Vision> }");
      if (vision.rows.length !== 1) throw Error("expected exactly one vision");
      const iri = vision.rows[0].v;
      await window.quipu.retract(iri);
      const zero = await window.quipu.query("SELECT ?p WHERE { ?v a <https://quipu.dev/knowledge/Vision> ; ?p ?o }");
      if (zero.rows.length) throw Error("Vision removal failed");
      return Array.from(await window.quipu.share());
    });
    await page.locator("#file-input").setInputFiles({ name: "without-vision.qpack.tar.gz",
      mimeType: "application/gzip", buffer: Buffer.from(pack) });
    await page.waitForFunction(() => document.querySelector("#status").textContent.includes("Loaded")
      && document.querySelector("#sparql-out").textContent.includes("No rows"), null, { timeout: 120000 });
    await verify(type);
    check("pack without vision preserves deep-linked entity", await current() === type);
    const invalid = new URL(page.url()); invalid.searchParams.set("entity", 'urn:bad> ?s ?p ?o');
    await page.goto(invalid.href); await initialAnswer();
    await page.waitForFunction(() => document.querySelector("#node-card").textContent.includes("Invalid entity IRI"));
    check("invalid deep link reports failure without stale facts", await page.locator("#node-card .node-links").count() === 0);
    const control = await page.evaluate(() => window.quipu.query("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1"));
    check("invalid IRI leaves real query path working", control.rows.length === 1);
    check("no browser errors", errors.length === 0, errors);
  } catch (error) {
    checks.push({ name: "terminal", ok: false, error: error.message }); process.exitCode = 1;
  } finally {
    fs.writeFileSync(path.join(out, "navigation.json"), JSON.stringify({ time: new Date().toISOString(),
      engine: await page.evaluate(() => window.__quipuBuild).catch(() => null), checks, errors }, null, 2));
    await context.close(); await browser.close(); server.close();
    if (!retained) fs.rmSync(out, { recursive: true, force: true });
  }
})().catch(async (error) => { console.error(error); await browser?.close(); server.close(); process.exitCode = 1; });
