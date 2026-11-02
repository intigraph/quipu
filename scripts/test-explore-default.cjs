// Real native snapshot + pack: pins, fresh queries, warm cache, refusal controls.
const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const fs = require("node:fs"), path = require("node:path"), http = require("node:http");
const site = path.resolve(process.env.SITE_DIR || "docs/book/src/explore");
let mode = "normal", snapshotFetches = 0;
const mime = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript",
  ".css": "text/css", ".wasm": "application/wasm", ".gz": "application/gzip" };
const server = http.createServer((request, response) => {
  try {
    let name = new URL(request.url, "http://localhost").pathname;
    if (name.endsWith("/")) name += "index.html";
    const file = path.resolve(site, "." + name);
    if (!file.startsWith(site + path.sep)) throw Error("outside site");
    let bytes = fs.readFileSync(file);
    if (name.endsWith("default-store.db.gz")) snapshotFetches++;
    const damaged = { pack: "repository.pendant.tar.gz", snapshot: "default-store.db.gz",
      engine: "quipu_wasm_explorer_bg.wasm" }[mode];
    if (damaged && name.endsWith(damaged)) { bytes = Buffer.from(bytes); bytes[bytes.length - 1] ^= 1; }
    if (mode === "shapes" && name.endsWith("default-pack-proof.js")) {
      bytes = Buffer.from(bytes.toString().replace(/"shapes_sha256":"[^"]+"/, '"shapes_sha256":"sha256:wrong"'));
    }
    if (name.endsWith("default-pack-proof.js")) {
      let text = bytes.toString();
      if (mode === "conforms") text = text.replace(/"conforms":(true|false)/,
        (_, value) => '"conforms":' + (value === "true" ? "false" : "true"));
      if (mode === "blocking") text = text.replace('"blocking":false', '"blocking":true');
      if (mode === "eligible") text = text.replace('"eligible":true', '"eligible":false');
      if (mode === "manifest") text = text.replace(/"graph_hash":"[^"]+"/, '"graph_hash":"sha256:wrong"');
      bytes = Buffer.from(text);
    }
    response.writeHead(200, { "content-type": mime[path.extname(file)] || "application/octet-stream",
      "cache-control": "no-store" });
    response.end(bytes);
  } catch { response.writeHead(404); response.end(); }
});
(async () => {
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const target = `http://127.0.0.1:${server.address().port}/`;
  const browser = await chromium.launch({ headless: true });
  let checks = 0;
  const check = (label, condition) => { assert.ok(condition, label); checks++; console.log("PASS", label); };
  async function open() {
    const context = await browser.newContext(), page = await context.newPage();
    await page.addInitScript(() => {
      const WorkerClass = window.Worker;
      window.__receiving = null; window.__progress = [];
      window.Worker = class extends WorkerClass {
        constructor(...args) {
          super(...args);
          this.addEventListener("message", event => {
            if (event.data.result?.receiving) window.__receiving = event.data.result.receiving;
            if (event.data.type === "quipu:load-progress") window.__progress.push(event.data.phase);
          });
        }
      };
    });
    return { context, page };
  }
  async function ready(page) {
    await page.waitForSelector('#graph-loader[data-state="ready"]', { state: "attached", timeout: 240000 });
    const result = await page.evaluate(() => window.quipu.query("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1"));
    check("ready has a real positive native query", result.rows.length === 1);
    return page.evaluate(() => ({ receiving: window.__receiving, readyMs: performance.now() }));
  }
  try {
    let { context, page } = await open();
    await page.goto(target);
    const cold = await ready(page);
    check("default uses producer-validated native store", cold.receiving.mode === "producer-validated");
    check("cold fetches one immutable snapshot", snapshotFetches === 1 && !cold.receiving.cached);
    await page.reload();
    const warm = await ready(page);
    check("warm uses the verified cached store", warm.receiving.cached && snapshotFetches === 1);
    const viewsMatch = await page.evaluate(async () => {
      const proof = (await import("./default-pack-proof.js")).default;
      const rows = result => result.rows.map(row => JSON.stringify(row)).sort();
      for (const name of ["knowledge", "files"]) {
        const live = await window.quipu.query(proof.views.queries[name]);
        if (JSON.stringify(rows(live)) !== JSON.stringify(rows(proof.views[name]))) return false;
      }
      return true;
    });
    check("published graph views equal independent live native queries", viewsMatch);
    const edited = await page.evaluate(async () => {
      const subject = (await window.quipu.query("SELECT (STR(?s) AS ?subject) WHERE { ?s <http://www.w3.org/2000/01/rdf-schema#label> ?label } LIMIT 1")).rows[0]?.subject;
      if (!subject) throw Error("missing label control");
      await window.quipu.set(subject, "http://www.w3.org/2000/01/rdf-schema#label", { str: "snapshot-cache-isolation-control" });
      return (await window.quipu.query('SELECT ?s WHERE { ?s <http://www.w3.org/2000/01/rdf-schema#label> "snapshot-cache-isolation-control" }')).rows.length;
    });
    check("restored store accepts a real native edit", edited === 1);
    await page.reload(); await ready(page);
    check("edits never enter the immutable default cache", await page.evaluate(async () =>
      (await window.quipu.query('SELECT ?s WHERE { ?s <http://www.w3.org/2000/01/rdf-schema#label> "snapshot-cache-isolation-control" }')).rows.length === 0));
    check("startup does not recompute export hashes", !(await page.locator("#export-manifest").innerText()).includes("RDFC-1.0"));
    // Tampering with cached bytes cannot mint a successful cached restore.
    await page.evaluate(() => new Promise((resolve, reject) => {
      const request = indexedDB.open("quipu-default-store-v1", 1);
      request.onerror = reject;
      request.onsuccess = () => {
        const db = request.result, tx = db.transaction("store", "readwrite"), store = tx.objectStore("store");
        const read = store.get("default");
        read.onsuccess = () => { const value = read.result; new Uint8Array(value.bytes)[0] ^= 1; store.put(value, "default"); };
        tx.oncomplete = () => { db.close(); resolve(); }; tx.onerror = reject;
      };
    }));
    await page.reload();
    const recovered = await ready(page);
    check("damaged cache refetches pinned bytes", !recovered.receiving.cached && snapshotFetches === 2);
    await page.locator("#pack-provenance").evaluate(node => { node.open = true; });
    await page.locator("#validate-pack").click();
    await page.waitForFunction(() => !document.querySelector("#validate-pack").disabled, null, { timeout: 240000 });
    check("explicit Validate pack completes full native checks", (await page.locator("#pack-validation-result").innerText()).includes("original pack")
      && !(await page.locator("#pack-validation-result").innerText()).includes("Validation failed"));
    // A selected file with the default's display name still takes full receiving.
    await page.locator("#file-input").setInputFiles({ name: "this site's copy of the latest release pack",
      mimeType: "application/gzip", buffer: fs.readFileSync(path.join(site, "repository.pendant.tar.gz")) });
    await page.waitForFunction(() => window.__receiving?.mode === "receiver-validated", null, { timeout: 240000 });
    await ready(page);
    check("selected pack performs full native SHACL", (await page.evaluate(() => window.__progress)).includes("verify_import"));
    await context.close();
    for (const failure of ["pack", "snapshot", "engine", "shapes", "conforms", "blocking", "eligible", "manifest"]) {
      mode = failure; ({ context, page } = await open());
      await page.goto(target);
      await page.waitForSelector('#graph-loader[data-state="error"]', { state: "attached", timeout: 30000 });
      check(`${failure} mismatch refuses readiness`, await page.locator("main").getAttribute("hidden") !== null
        && await page.evaluate(() => window.__receiving === null));
      await context.close();
    }
    console.log(JSON.stringify({ checks, cold, warm }));
  } finally { await browser.close(); await new Promise(resolve => server.close(resolve)); }
})().catch(error => { console.error(error); process.exitCode = 1; });
