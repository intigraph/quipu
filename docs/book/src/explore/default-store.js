// Only this Pages build's immutable, producer-validated store gets the fast path.
import proof from "./default-pack-proof.js";
import { constellationQueries } from "./constellation-queries.js";
const digest = async bytes => "sha256:" + [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))]
  .map(byte => byte.toString(16).padStart(2, "0")).join("");
const slot = "quipu-default-store-v1";

export async function initializeEngine(init) {
  if (!proof) { await init(); return null; }
  const response = await fetch(new URL("./pkg/quipu_wasm_explorer_bg.wasm", import.meta.url));
  if (!response.ok) throw Error("Engine download failed");
  const wasm = await response.arrayBuffer();
  if (await digest(wasm) !== proof.wasm_sha256) throw Error("Engine differs from this site's validated build");
  await init({ module_or_path: wasm });
  return proof.wasm_sha256;
}

async function cache(action, bytes) {
  if (!globalThis.indexedDB) return null;
  return new Promise(resolve => {
    let done = false, db;
    const finish = result => { if (!done) { done = true; db?.close(); resolve(result); } };
    const timer = setTimeout(() => finish(null), 500);
    let request;
    try { request = indexedDB.open(slot, 1); }
    catch { clearTimeout(timer); finish(null); return; }
    request.onupgradeneeded = () => request.result.createObjectStore("store");
    request.onerror = () => { clearTimeout(timer); finish(null); };
    request.onsuccess = () => {
      db = request.result;
      if (done) { db.close(); return; }
      try {
        const tx = db.transaction("store", action === "read" ? "readonly" : "readwrite");
        const store = tx.objectStore("store");
        const op = action === "read" ? store.get("default")
          : store.put({ identity: proof.snapshot_sha256, bytes }, "default");
        op.onsuccess = () => {
          if (action === "read") { clearTimeout(timer); finish(op.result); }
        };
        tx.oncomplete = () => { clearTimeout(timer); finish(null); };
        tx.onerror = () => { clearTimeout(timer); finish(null); };
      } catch { clearTimeout(timer); finish(null); }
    };
  });
}

export async function restoreDefault(Explorer, pack, source, engineIdentity, progress) {
  if (!proof || typeof Explorer.restoreVerifiedSnapshot !== "function") return null;
  if (proof.schema !== "quipu.explorer-default/v1" || typeof proof.conforms !== "boolean"
      || proof.conforms !== proof.report?.import?.validation?.conforms
      || proof.shapes_sha256 !== proof.report?.manifest?.shapes_hash
      || proof.wasm_sha256 !== engineIdentity || proof.snapshot !== "./default-store.db.gz") throw Error("Invalid default-pack validation proof");
  progress("verify_default");
  if (await digest(pack) !== proof.pack_sha256) throw Error("Default pack differs from this site's validated pack");
  const saved = await cache("read");
  let compressed = saved?.identity === proof.snapshot_sha256 && saved.bytes instanceof ArrayBuffer
    ? saved.bytes : null;
  let cached = !!compressed;
  if (compressed && await digest(compressed) !== proof.snapshot_sha256) { compressed = null; cached = false; }
  if (!compressed) {
    const response = await fetch(new URL(proof.snapshot, import.meta.url));
    if (!response.ok) throw Error("Validated graph download failed");
    compressed = await response.arrayBuffer();
    if (await digest(compressed) !== proof.snapshot_sha256) throw Error("Validated graph digest mismatch");
    // Only immutable producer bytes are saved. Edits never enter this cache.
    await cache("write", compressed);
  }
  progress("restore_default");
  const snapshot = await new Response(new Blob([compressed]).stream()
    .pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
  const explorer = Explorer.restoreVerifiedSnapshot(new Uint8Array(snapshot),
    new Uint8Array(pack), source, JSON.stringify(proof.report));
  const queries = constellationQueries();
  const views = proof.views?.queries?.knowledge === queries.knowledge
    && proof.views?.queries?.files === queries.files ? proof.views : null;
  return { explorer, views, receiving: { mode: "producer-validated", cached,
    pack_sha256: proof.pack_sha256, shapes_sha256: proof.shapes_sha256 } };
}
