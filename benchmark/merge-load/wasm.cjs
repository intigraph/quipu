// Execute the actual shipped Explorer binding under Node's real WebAssembly VM.
// This measures offline engine/delta semantics, not browser DOM or OPFS latency.
const fs = require('node:fs');
globalThis.crypto ??= require('node:crypto').webcrypto;
const [modulePath, archive, editsFile, output] = process.argv.slice(2);
const {Explorer} = require(modulePath);
const edits = JSON.parse(fs.readFileSync(editsFile, 'utf8'));
const start = performance.now();
const ex = Explorer.loadPendant(fs.readFileSync(archive), 'merge-load-fixture', '2026-10-20T00:00:00Z');
const load = performance.now();
for (const [s,p] of edits.remove) {
  const result = JSON.parse(ex.retract(s,p,''));
  if (result.conforms === false) throw new Error('retraction refused');
}
if (edits.insert) {
  const result = JSON.parse(ex.knot(JSON.stringify({turtle:edits.insert})));
  if (result.conforms === false) throw new Error('addition refused');
}
const changed = performance.now();
const delta = JSON.parse(ex.delta());
if (delta.empty) throw new Error('delta unexpectedly empty');
fs.mkdirSync(output);
for (const f of delta.files) {
  if (!/^[a-zA-Z0-9_.-]+$/.test(f.name)) throw new Error('unsafe delta member');
  fs.writeFileSync(`${output}/${f.name}`,f.contents);
}
fs.writeFileSync(`${output}/load-metrics.json`,JSON.stringify({runtime:'Node WebAssembly; actual Explorer.delta()',
  load_ms:load-start,edit_ms:changed-load,delta_ms:performance.now()-changed,delta_bytes:delta.total_bytes,
  edit_log_entries:JSON.parse(ex.editLog()).length}));
ex.free();
