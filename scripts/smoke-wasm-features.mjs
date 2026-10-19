// Actual browser/worker feature acceptance against a native-produced pack.
import { chromium } from "../wasm/explorer/runtime/node_modules/playwright/index.mjs";
import { createServer } from "node:http";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { join, resolve, extname } from "node:path";
import { tmpdir } from "node:os";
import assert from "node:assert/strict";
import { featureFixture } from "./wasm-feature-fixture.mjs";

const repo = resolve(import.meta.dirname, "..");
const bundle = resolve(process.argv[2] ?? "wasm/explorer/pkg");
const runtime = resolve(process.argv[3] ?? "wasm/explorer/runtime/dist");
const work = mkdtempSync(join(tmpdir(), "quipu-wasm-features-"));
const binary = process.env.QUIPU_BIN ?? join(repo, "target/release/quipu");
const fixture = featureFixture(repo, binary, work);
const requests = [], remoteBodies = [];
const malformedResponses = [null, true, 1, "invalid", [], ["invalid"], {},
  { variables: [], rows: null }, { variables: [1], rows: [] },
  { variables: [], rows: [null] }, { variables: [], rows: [[]] }];
// Exercise the shipped worker, not a parallel implementation of its commands.
const worker = readFileSync(join(repo, "docs/book/src/explore/worker.js"), "utf8");
const html = `<!doctype html><title>Private WASM feature acceptance</title><script type="module">
import { createEngineApi } from "/engine-api.js";
const worker=new Worker('/worker.mjs',{type:'module'}); const pending=new Map(); let seq=0;
worker.onmessage=({data:m})=>{const p=pending.get(m.id);pending.delete(m.id);p.resolve({...m,value:m.result})};
worker.onerror=e=>{for(const p of pending.values())p.reject(new Error(e.message))};
window.ask=msg=>new Promise((resolve,reject)=>{const id=++seq;pending.set(id,{resolve,reject});worker.postMessage({...msg,id})});
window.quipu=createEngineApi(async msg=>{
 const answer=await window.ask(msg);
 if(!answer.ok) throw Object.assign(new Error(answer.error),{validation:answer.validation});
 return answer.value;
});
</script>`;
const server = createServer(async (req, res) => {
  const pathname = new URL(req.url, "http://localhost").pathname;
  requests.push(pathname);
  if (pathname.startsWith("/page/")) {
    const relative = pathname.slice("/page/".length) || "index.html";
    const root = relative.startsWith("pkg/runtime/") ? runtime : relative.startsWith("pkg/") ? bundle : join(repo,"docs/book/src/explore");
    const file = relative.startsWith("pkg/runtime/") ? relative.slice(12) : relative.startsWith("pkg/") ? relative.slice(4) : relative;
    const target = resolve(root, file);
    if (!target.startsWith(`${resolve(root)}/`)) { res.writeHead(403); res.end(); return; }
    try {
      const body = relative === "repository.pendant.tar.gz" ? readFileSync(fixture.pack) : readFileSync(target);
      res.setHeader("Content-Type", /\.m?js$/.test(file) ? "text/javascript" : file.endsWith(".wasm") ? "application/wasm" : file.endsWith(".html") ? "text/html" : file.endsWith(".css") ? "text/css" : "application/octet-stream");
      res.end(body);
    } catch { res.writeHead(404); res.end(); }
    return;
  }
  if (pathname === "/") { res.setHeader("Content-Type", "text/html"); res.end(html); return; }
  if (pathname === "/engine-api.js") { res.setHeader("Content-Type", "text/javascript"); res.end(readFileSync(join(repo,"docs/book/src/explore/engine-api.js"))); return; }
  if (pathname === "/worker.mjs") { res.setHeader("Content-Type", "text/javascript"); res.end(worker); return; }
  if (pathname.endsWith("/query")) {
    let bytes = ""; for await (const chunk of req) bytes += chunk;
    remoteBodies.push(JSON.parse(bytes));
    if (pathname === "/failed/query") { res.writeHead(503); res.end("unavailable"); return; }
    if (pathname === "/slow/query") { setTimeout(() => res.end("{}"), 250); return; }
    res.setHeader("Content-Type", "application/json");
    const malformed = /^\/malformed-(\d+)\/query$/.exec(pathname);
    if (malformed) { res.end(JSON.stringify(malformedResponses[Number(malformed[1])])); return; }
    res.end(JSON.stringify({ variables: ["s"], rows: [{ s: "http://example.org/remote" }] })); return;
  }
  const root = pathname.startsWith("/pkg/runtime/") ? runtime : bundle;
  const relative = pathname.replace(/^\/pkg\/(runtime\/)?/, "");
  const file = resolve(root, relative);
  if (!file.startsWith(`${root}/`)) { res.writeHead(403); res.end(); return; }
  try {
    const body = readFileSync(file);
    res.setHeader("Content-Type", [".js", ".mjs"].includes(extname(file)) ? "text/javascript" :
      extname(file) === ".wasm" ? "application/wasm" : "application/json");
    res.end(body);
  } catch { res.writeHead(404); res.end(); }
});

let browser;
try {
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const base = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ headless: !process.argv.includes("--headed"),
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const page = await browser.newPage();
  const errors = []; page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(base);
  await page.waitForFunction(() => window.quipu && window.ask);
  const ask = (input) => {
    const message = { ...input };
    if (message.cmd === "semantic") message.cmd = "search";
    if (message.cmd === "episode") { message.episode = JSON.stringify(message.input); delete message.input; }
    if (message.cmd === "ontology") message.input = JSON.stringify(message.input);
    if (["knot", "shapes"].includes(message.cmd)) message.input = JSON.stringify(message.input);
    if (["set", "retract"].includes(message.cmd)) message.value = JSON.stringify(message.value);
    return page.evaluate((m) => window.ask(m), message);
  };
  const ok = async (input) => { const result = await ask(input); assert.equal(result.ok, true, result.error); return result.value; };
  const loaded = await ok({ cmd: "load", source: "feature-fixture", bytes: Array.from(readFileSync(fixture.pack)) });
  assert.equal(loaded.shacl_compiled, true);
  assert.equal(loaded.import.validation.conforms, true);
  assert.equal(loaded.promotion.outcome, "promoted");
  assert.deepEqual((await ok({ cmd: "version" })).compiled_features, { shacl: true, owl: true, reactive_reasoner: true });
  assert.equal((await ok({ cmd: "capabilities" })).state, "not_loaded");
  assert.equal(requests.filter((url) => /transformers|\/models\/|ort-wasm/.test(url)).length, 0);
  console.log("PASS real pack load, compiled flags, no heavy assets before search");

  const bad = await ask({ cmd: "episode", input: { name: "invalid-widget", source: "feature-control",
    nodes: [{ name: "invalid-pattern", type: "InternalIdentifierPattern" }] } });
  assert.equal(bad.ok, false); assert.match(bad.error, /SHACL/);
  assert.ok(bad.validation?.violations > 0); assert.ok(bad.validation.messages.length > 0);
  const widgets = "SELECT ?s WHERE { ?s a <http://example.org/Widget> }";
  assert.equal((await ok({ cmd: "query", sparql: widgets })).rows.length, 2);
  const validation = await ok({ cmd: "validate", shapes: readFileSync(fixture.shapes, "utf8"),
    data: "<http://example.org/bad> a <http://example.org/Widget> ." });
  assert.equal(validation.conforms, false); assert.ok(validation.results.length > 0);
  console.log("PASS violating write refused with real feedback; existing two widgets preserved");

  const extraShape = `@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:binding:RecordShape> a sh:NodeShape; sh:targetClass <urn:binding:Record>;
 sh:property [sh:path <urn:binding:title>; sh:minCount 1] .`;
  await ok({ cmd: "shapes", input: { action: "load", name: "browser-store-ops", turtle: extraShape } });
  assert.equal((await ok({ cmd: "shapes", input: { action: "get", name: "browser-store-ops" } })).turtle, extraShape);
  const goodKnot = await ok({ cmd: "knot", input: {
    turtle: '<urn:binding:good> a <urn:binding:Record>; <urn:binding:title> "Before" .',
  } });
  assert.ok(goodKnot.tx_id > 0);
  const editsBeforeBadKnot = (await ok({ cmd: "editLog" })).length;
  const badKnot = await ask({ cmd: "knot", input: { turtle: '<urn:binding:bad> a <urn:binding:Record> .' } });
  assert.ok(badKnot.ok === false || badKnot.value?.conforms === false);
  assert.equal((await ok({ cmd: "query", sparql: 'SELECT ?p WHERE { <urn:binding:bad> ?p ?v }' })).rows.length, 0);
  assert.equal((await ok({ cmd: "editLog" })).length, editsBeforeBadKnot);
  await ok({ cmd: "set", entity: "urn:binding:good", predicate: "urn:binding:title", value: { str: "After" } });
  const history = await ok({ cmd: "entityHistory", iri: "urn:binding:good" });
  assert.ok(history.history.some((fact) => fact.value === "Before"));
  assert.ok(history.history.some((fact) => fact.value === "After"));
  assert.equal((await ask({ cmd: "entityHistory", iri: "urn:binding:absent" })).ok, false);
  console.log("PASS shipped worker shape adoption, knot admission/refusal/no-leak and native entity history");

  const edge = (predicate, subject = "car", object = "cat") =>
    `SELECT ?s FROM <urn:quipu:graph:root> FROM <urn:quipu:graph:root#inferred> WHERE { <http://example.org/${subject}> <http://example.org/${predicate}> ?s . FILTER(?s=<http://example.org/${object}>) }`;
  assert.equal((await ok({ cmd: "query", sparql: edge("derivedBack") })).rows.length, 0);
  await ok({ cmd: "set", entity: "http://example.org/cat", predicate: "http://example.org/connects", value: { iri: "http://example.org/car" } });
  assert.equal((await ok({ cmd: "query", sparql: edge("derivedBack") })).rows.length, 1);
  await ok({ cmd: "ontology", input: { action: "load", name: "inverse-control",
    turtle: "<http://example.org/connects> <http://www.w3.org/2002/07/owl#inverseOf> <http://example.org/owlBack> ." } });
  assert.equal((await ok({ cmd: "query", sparql: edge("owlBack") })).rows.length, 1);
  await ok({ cmd: "set", entity: "http://example.org/car", predicate: "http://example.org/connects", value: { iri: "http://example.org/cat" } });
  assert.equal((await ok({ cmd: "query", sparql: edge("owlBack", "cat", "car") })).rows.length, 1);
  console.log("PASS adopted Datalog 0->1 and OWL load plus later reactive inverse write");

  const result = await page.evaluate(() => window.quipu.search("a pet cat playing", 2));
  // Preserve the engine's compact term representation in search results.
  assert.equal(result[0].entity, "ex:cat"); assert.ok(result[0].score > result[1].score);
  assert.ok(requests.some((url) => url.endsWith("model_quantized.onnx")));
  console.log("PASS semantic ranking over actual ROOT data with self-hosted ONNX");

  const sparql = "SELECT ?s WHERE { ?s a <http://example.org/Widget> }";
  const remote = { name: "peer", endpoint: `${base}/peer`, label: { freshness: "fresh" } };
  const wire = await ok({ cmd: "remote", options: remote, sparql, graph: "urn:fixture:selected" });
  assert.equal(wire.declared_label.freshness, "fresh"); assert.equal(remoteBodies.at(-1).graph, "urn:fixture:selected");
  const full = await ok({ cmd: "federate", remotes: [remote], floor: {}, sparql });
  assert.equal(full.complete, true); assert.equal(full.result.rows.length, 3);
  const partial = await ok({ cmd: "federate", remotes: [remote, { name: "failed", endpoint: `${base}/failed` }], floor: {}, sparql });
  assert.equal(partial.complete, false); assert.equal(partial.providers.find((p) => p.name === "failed").ok, false);
  for (const [index, payload] of malformedResponses.entries()) {
    const malformed = { name: "malformed", endpoint: `${base}/malformed-${index}` };
    if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
      const refused = await ask({ cmd: "remote", options: malformed, sparql });
      assert.equal(refused.ok, false); assert.match(refused.error, /response must be an object/);
    }
    const degraded = await ok({ cmd: "federate", remotes: [remote, malformed], floor: {}, sparql });
    assert.equal(degraded.complete, false);
    assert.equal(degraded.providers.find((p) => p.name === "malformed").ok, false);
    assert.equal(degraded.providers.find((p) => p.name === "local").ok, true);
    assert.equal(degraded.providers.find((p) => p.name === "peer").ok, true);
    assert.equal(degraded.result.rows.length, 3);
  }
  console.log("PASS malformed null/scalar/array/SELECT members preserve local and valid peer rows");
  const count = remoteBodies.length;
  const floor = await ask({ cmd: "federate", remotes: [remote], floor: { min_freshness: "fresh" }, sparql });
  assert.equal(floor.ok, false); assert.equal(remoteBodies.length, count);
  assert.equal((await ok({ cmd: "query", sparql })).rows.length, 2);
  const timeout = await ask({ cmd: "remote", options: { name: "slow", endpoint: `${base}/slow`, timeoutMs: 10 }, sparql });
  assert.equal(timeout.ok, false);
  console.log("PASS fetch selected graph, federation completeness, native floor before network, timeout");
  await page.close();
  const actual = await browser.newPage(process.env.BROWSER_DEMO_DIR
    ? { viewport: {width:1280,height:960}, recordVideo: {dir:process.env.BROWSER_DEMO_DIR} } : {});
  actual.on("pageerror", error => errors.push(error.message));
  await actual.goto(`${base}/page/`);
  await actual.waitForFunction(() => /^Loaded /.test(document.querySelector("#status")?.textContent ?? ""));
  const beforeSearch = await actual.evaluate(() => window.quipu.capabilities());
  assert.equal(beforeSearch.state, "not_loaded");
  assert.equal(requests.filter(url=>url.startsWith("/page/") && /transformers|\/models\/|ort-wasm/.test(url)).length,0);
  const rejection = await actual.evaluate(async () => {
    try { await window.quipu.episode({name:"page-invalid",source:"page-control",nodes:[{name:"invalid-page-policy",type:"InternalIdentifierPattern"}]}); }
    catch(error) { return { message:error.message,validation:error.validation }; }
  });
  assert.match(rejection.message,/SHACL/); assert.ok(rejection.validation.violations > 0);
  await actual.locator("#semantic-query").fill("a pet cat playing");
  await actual.locator("#semantic-run").click();
  await actual.waitForFunction(() => document.querySelector("#semantic-results p strong"));
  assert.match(await actual.locator("#semantic-results p").first().innerText(),/ex:cat/);
  await actual.waitForFunction(() => navigator.serviceWorker.controller !== null);
  const http = await actual.evaluate(async () => {
    const response=await fetch("./search",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({query:"a pet cat playing",limit:2})});
    return {status:response.status,body:await response.json()};
  });
  assert.equal(http.status,200); assert.equal(http.body[0].entity,"ex:cat");
  const invalidHttp = await actual.evaluate(async () => {
    const response=await fetch("./episode",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({name:"http-invalid",source:"page-control",nodes:[{name:"invalid-http-policy",type:"InternalIdentifierPattern"}]})});
    return {status:response.status,body:await response.json()};
  });
  assert.equal(invalidHttp.status,500); assert.ok(invalidHttp.body.validation.violations > 0);
  await actual.evaluate(() => window.quipu.set("http://example.org/cat","http://www.w3.org/2000/01/rdf-schema#comment",{str:"A kitten purrs"}));
  const written=await actual.evaluate(() => window.quipu.query('SELECT ?s WHERE { ?s <http://www.w3.org/2000/01/rdf-schema#comment> "A kitten purrs" }'));
  assert.equal(written.rows.length,1);
  if (process.env.BROWSER_DEMO_DIR) await actual.screenshot({path:join(process.env.BROWSER_DEMO_DIR,"workbench.png"),fullPage:true});
  console.log("PASS actual workbench search UI/window/HTTP, structured SHACL errors on both relays, valid write readback");
  assert.deepEqual(errors, []);
  console.log("WASM full browser feature acceptance: all checks passed");
} finally {
  if (browser) await browser.close();
  server.close();
  rmSync(work, { recursive: true, force: true });
}
