// Actual browser engine; temporary edits affect only this tab's receiving store.
const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const site = path.resolve(process.env.SITE_DIR || "docs/book/src/explore");
const out = process.env.OUT_DIR || "/tmp/quipu-cords";
const demoPause = () => new Promise(r => setTimeout(r,
  Math.min(2000, Math.max(0, Number(process.env.DEMO_PAUSE_MS) || 0))));
fs.mkdirSync(out, {recursive:true});
const server = http.createServer((req,res) => {
  const name = new URL(req.url,"http://localhost").pathname;
  const file = path.resolve(site,"."+(name.endsWith("/")?name+"index.html":name));
  if (!file.startsWith(site+path.sep)) { res.writeHead(403).end(); return; }
  try { res.setHeader("content-type", ({".js":"text/javascript",".mjs":"text/javascript",".wasm":"application/wasm",".css":"text/css",".html":"text/html"})[path.extname(file)]||"application/octet-stream");res.end(fs.readFileSync(file)); }
  catch { res.writeHead(404).end(); }
});
(async()=>{
  await new Promise(r=>server.listen(0,"127.0.0.1",r));
  const browser=await chromium.launch({headless:!process.argv.includes("--headed"),
    ...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}: {})});
  const page=await browser.newPage({viewport:{width:1280,height:900},reducedMotion:"reduce",recordVideo:{dir:out,size:{width:1280,height:900}}});
  const errors=[];page.on("pageerror",e=>errors.push(e.message));
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/?embed=1&theme=intigraph`);
    await page.waitForSelector("#node-card .entity-types",{timeout:120000});
    assert(await page.locator("#graph [data-cord=main]").count());
    assert(await page.locator("#graph [data-cord=pendant]").count());
    assert.match(await page.locator("#graph-legend").textContent(),/degree.*loaded view/);
    const size=await page.locator("#graph circle").evaluateAll(ns=>[...new Set(ns.map(n=>n.getAttribute("r")))]);
    assert(size.length>1,"degree must change knot size in populated graph");
    const index=await page.locator("#graph [data-edge]").evaluateAll(lines=>lines.findIndex(line=>{
      const e=JSON.parse(line.dataset.edge);return ["https://w3id.org/intigraph/quipu/id/idea/vision","https://quipu.dev/knowledge/vision"].includes(e.from)
        && e.rel==="https://quipu.dev/knowledge/guides";
    }));
    assert(index>=0,"positive control: vision guides a decision");
    const edge=await page.locator("#graph [data-edge]").nth(index).getAttribute("data-edge");
    const expected=JSON.parse(edge);
    // Keyboard focus alone is insufficient: an aria-hidden ancestor can make
    // an otherwise working relation control invisible to assistive technology.
    const cdp=await page.context().newCDPSession(page);
    const {root}=await cdp.send("DOM.getDocument");
    const {nodeId}=await cdp.send("DOM.querySelector",{nodeId:root.nodeId,selector:"#graph [data-edge]"});
    const ax=await cdp.send("Accessibility.getPartialAXTree",{nodeId,fetchRelatives:false});
    assert.equal(ax.nodes[0].ignored,false,"relation must be exposed in the accessibility tree");
    assert.equal(ax.nodes[0].role.value,"button");
    assert(ax.nodes[0].name.value.includes("→"),"relation has named subject and object");
    await cdp.detach();
    await page.locator("#graph [data-edge]").nth(index).focus();
    await page.keyboard.press("Enter");
    await page.waitForSelector("#node-card .relation-history");
    await page.waitForFunction(()=>document.querySelector("#node-card").getAttribute("aria-busy")==="false");
    assert.equal(await page.evaluate(()=>document.activeElement===document.querySelector("#node-card h3")),
      true,"Enter moves keyboard focus into relation details");
    const readable=await page.locator("#node-card dd").evaluateAll(values=>values.every(dd=>dd.clientWidth>120));
    assert(readable,"relation values retain a readable column width");
    const text=await page.locator("#node-card").textContent();
    for(const value of Object.values(expected)) assert(text.includes(value),value);
    assert(text.includes("Receiver history"));assert(text.includes("not assertions"));
    await page.locator(".relation-history summary").click();
    await demoPause();
    const history=await page.evaluate(s=>window.quipu.factHistory(s),expected.from);
    const matching=history.facts.filter(f=>f.predicate===expected.rel&&f.object.kind==="iri"&&f.object.value===expected.to);
    assert(matching.length,"positive control: actual receiver fact history");
    for(const f of matching) {assert(text.includes(f.valid_from));assert(text.includes(f.transaction_time));}
    await page.locator("#constellation").scrollIntoViewIfNeeded();
    await page.screenshot({path:path.join(out,"relation-desktop.png")});
    await demoPause();
    // Close/reassert one existing IRI relation using the real engine's local write path.
    await page.evaluate(async e=>{await window.quipu.retract(e.from,e.rel,{iri:e.to});
      await window.quipu.set(e.from,e.rel,{iri:e.to});},expected);
    const changed=await page.evaluate(s=>window.quipu.factHistory(s),expected.from);
    assert(changed.facts.some(f=>f.operation==="Retract"&&f.predicate===expected.rel&&f.object.value===expected.to));
    assert(changed.facts.length>history.facts.length,"append-only receiver history");
    // Full IRI navigation from the relation panel, then per-relation list controls.
    await page.locator("#node-card dl button").first().click();
    await page.waitForSelector("#node-card .entity-types");
    await page.locator("#node-card .node-links a").first().click();
    await page.waitForSelector("#node-card .relation-history");
    await page.setViewportSize({width:375,height:844});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.screenshot({path:path.join(out,"relation-mobile.png"),fullPage:true});
    await demoPause();
    await page.click("#graph-fit");
    // ResizeObserver redraws on the next frame; inspect settled geometry,
    // rather than a transient mix of old positions and new viewport width.
    await page.waitForFunction(()=>{
      const svg=document.querySelector("#graph");
      return svg.viewBox.baseVal.width===svg.clientWidth;
    });
    await page.evaluate(async()=>{await document.fonts.ready;
      await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));});
    const targets=await page.locator("#graph-targets .cluster-target").evaluateAll(bs=>bs
      .map(b=>{const r=b.getBoundingClientRect();return {left:r.left,right:r.right,width:r.width,height:r.height}})
      .sort((a,b)=>a.left-b.left));
    assert(targets.every((r,i)=>r.width>=44&&r.height>=44&&(!i||targets[i-1].right<=r.left)),
      "375px cord targets retain separate 44px touch areas: "+JSON.stringify(targets));
    await page.screenshot({path:path.join(out,"cord-overview.png")});
    await demoPause();
    assert(await page.locator("#graph [data-cord=main]").count());
    // Missing term and injection-shaped strings never create terms or leak all history.
    const missing=await page.evaluate(()=>window.quipu.factHistory("urn:missing:no-history"));
    assert.deepEqual(missing.facts,[]);
    assert.deepEqual(errors,[]);
    console.log("PASS cords, degree, keyboard edge detail, exact relation times, append-only retract history, endpoint/list navigation, mobile/reduced motion, missing-history control");
  } finally {await browser.close();await new Promise(r=>server.close(r));}
})().catch(e=>{console.error(e);server.close();process.exitCode=1});
