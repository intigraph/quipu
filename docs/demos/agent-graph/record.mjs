// Records the Quipu README demo: an agent's commands on the left, the live graph on the right.
// Every command shown is really run against a fresh local store; every output is its real output.
// usage: npm install && node record.mjs <quipu-bin-dir> <out-dir>   (CHROME=<path> overrides the browser)
import { chromium } from 'playwright-core';
import { spawn, execFileSync } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const [binDir, outDir] = process.argv.slice(2);
const here = path.dirname(new URL(import.meta.url).pathname);
const scen = path.join(here, 'scenario');
const work = fs.mkdtempSync('/tmp/quipu-demo-');
const db = path.join(work, 'demo.db');
const QP = path.join(binDir, 'quipu');
const STAGE_PORT = 47930, QUIPU_PORT = 47931;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Files the commands read, copied so the terminal shows short relative names.
for (const f of fs.readdirSync(scen)) if (f.endsWith('.ttl')) fs.copyFileSync(path.join(scen, f), path.join(work, f));
execFileSync(QP, ['shapes', 'load', 'shop', 'shapes.ttl', '--db', db], { cwd: work });

const server = spawn(path.join(binDir, 'quipu-server'), ['--db', db, '--bind', `127.0.0.1:${QUIPU_PORT}`], { stdio: 'ignore' });
const stage = http.createServer((req, res) => {
  const p = path.join(here, decodeURIComponent(new URL(req.url, 'http://x').pathname));
  if (!p.startsWith(here) || !fs.existsSync(p) || fs.statSync(p).isDirectory()) { res.writeHead(404); return res.end(); }
  const type = p.endsWith('.html') ? 'text/html' : p.endsWith('.css') ? 'text/css' : 'text/javascript';
  res.writeHead(200, { 'content-type': type }); fs.createReadStream(p).pipe(res);
}).listen(STAGE_PORT, '127.0.0.1');

function run(args) {
  try { return { ok: true, out: execFileSync(QP, [...args, '--db', db], { cwd: work, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }) }; }
  catch (e) { return { ok: false, out: (e.stdout || '') + (e.stderr || '') }; }
}

const browser = await chromium.launch({ executablePath: process.env.CHROME || '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const ctx = await browser.newContext({ viewport: { width: 1600, height: 900 }, recordVideo: { dir: outDir, size: { width: 1600, height: 900 } } });
const page = await ctx.newPage();
let capturing = false, t0 = 0, capture = Promise.resolve();
try {
  await sleep(1500);
  await page.goto(`http://127.0.0.1:${STAGE_PORT}/stage.html`);
  await page.evaluate((u) => window.stage.graph(u), `http://127.0.0.1:${QUIPU_PORT}/ui#explorer`);
  await sleep(2500);
  // Frames for the README GIF: a screenshot every 250ms, named by elapsed milliseconds.
  const frames = path.join(outDir, 'frames'); fs.mkdirSync(frames, { recursive: true });
  capturing = true; t0 = Date.now();
  capture = (async () => { while (capturing) {
    const t = Date.now() - t0;
    await page.screenshot({ path: path.join(frames, `${String(t).padStart(7, '0')}.png`) }).catch(() => {});
    await sleep(Math.max(0, 250 - (Date.now() - t0 - t)));
  } })();
  const frame = () => page.frames().find((f) => f.url().includes(`:${QUIPU_PORT}/`));
  const refresh = async () => { await frame().evaluate(() => { if (typeof loadEntities === 'function') loadEntities(); if (typeof checkHealth === 'function') checkHealth(); }); await sleep(1800); };
  const say = async (text) => {
    await page.evaluate(() => window.stage.write('\r\n\x1b[38;5;209m● \x1b[0m'));
    for (const ch of text) { await page.evaluate((c) => window.stage.write(c), ch); await sleep(18); }
    await page.evaluate(() => window.stage.write('\r\n')); await sleep(700);
  };
  const cmd = async (args, shown) => {
    await page.evaluate((s) => window.stage.write(`\x1b[2m$ \x1b[0m${s}\r\n`), shown || `qp ${args.join(' ')}`);
    await sleep(500);
    const r = run(args);
    const color = r.ok ? '' : '\x1b[31m';
    await page.evaluate(([o, c]) => window.stage.write(c + o.trimEnd().replace(/\n/g, '\r\n') + '\x1b[0m\r\n'), [r.out, color]);
    return r;
  };
  const caption = (s) => page.evaluate((t) => window.stage.caption(t), s);
  let shot = 0;
  const snap = async () => { await page.screenshot({ path: path.join(outDir, `step-${++shot}.png`) }); };

  await caption('1. The agent records what it learned, with its source.');
  await say('I traced a checkout request. Recording the services it touched, citing the trace.');
  await cmd(['knot', '1-learn.ttl', '--shapes', 'shapes.ttl', '--timestamp', '2026-01-01T09:00:00Z'], 'qp knot 1-learn.ttl --shapes shapes.ttl');
  await refresh(); await snap(); await sleep(2500);

  await caption('2. A guess with no source is refused. Nothing enters the graph.');
  await say('The cache probably sits in front of orders. Saving that.');
  const g = await cmd(['knot', '2-guess.ttl', '--shapes', 'shapes.ttl', '--timestamp', '2026-01-02T09:00:00Z'], 'qp knot 2-guess.ttl --shapes shapes.ttl');
  if (g.ok) throw new Error('the guess was accepted; the demo would show a false refusal');
  await page.evaluate(() => window.stage.flash()); await sleep(300); await snap(); await refresh(); await sleep(2500);

  await caption('3. Corrected with provenance, it is accepted.');
  await say('Refused: no source. Trace 1107 confirms it, so I record it as observed.');
  await cmd(['knot', '3-correct.ttl', '--shapes', 'shapes.ttl', '--timestamp', '2026-01-02T10:00:00Z'], 'qp knot 3-correct.ttl --shapes shapes.ttl');
  await refresh(); await snap(); await sleep(2500);

  await caption('4. Facts change. History stays.');
  await say('orders-db moved from db-a to db-b on January 3.');
  await cmd(['retract', 'https://example.org/shop/orders-db', '--predicate', 'https://example.org/shop/runsOn'], 'qp retract shop:orders-db --predicate shop:runsOn');
  await cmd(['knot', '4-migrate.ttl', '--timestamp', '2026-01-03T09:00:00Z'], 'qp knot 4-migrate.ttl');
  await refresh(); await sleep(1500);

  await caption('5. Ask what was true before the move.');
  await say('Where did orders-db run on January 2?');
  const q = 'SELECT ?host WHERE { <https://example.org/shop/orders-db> <https://example.org/shop/runsOn> ?host }';
  await cmd(['read', q, '--valid-at', '2026-01-02T12:00:00Z'], 'qp read "SELECT ?host ..." --valid-at 2026-01-02');
  await say('And now?');
  await cmd(['read', q], 'qp read "SELECT ?host ..."');
  await snap();
  await caption('Governed memory: refused writes stay out; every fact keeps its source and its time.');
  await sleep(4000);
} finally {
  capturing = false; await capture;
  await ctx.close(); await browser.close(); server.kill(); stage.close();
  fs.rmSync(work, { recursive: true, force: true });
}
console.log('recorded to', outDir);
