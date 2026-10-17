// Host transport for PointerExplorer. Call only when a consumer needs a graph.
// authorize(pointer) is receiver policy (for example a Caboodle policy exported
// by a trusted host). A manifest never supplies this callback or its answer.
const MAX_BYTES = 16 * 1024 * 1024;

async function boundedBytes(response) {
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  if (!response.body) throw new Error('archive response has no body');
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > MAX_BYTES) throw new Error('archive exceeds download byte limit');
      chunks.push(value);
    }
  } catch (error) {
    await reader.cancel().catch(() => {});
    throw error;
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return bytes;
}

/** Resolve one graph. cache is an optional receiver-owned CacheStorage Cache.
 * Cache entries are reverified by Rust; bad entries refuse, never trigger a
 * silent refetch. No fetch/cache access happens until current trust succeeds.
 */
export async function usePointerGraph(engine, graph, authorize, { cache, fetcher = fetch } = {}) {
  try {
    const pointer = JSON.parse(engine.pointer(graph));
    if (await authorize(pointer) !== true) throw new Error('receiver pointer trust refused');
    const timestamp = new Date().toISOString();
    if (engine.hasCached(graph)) return JSON.parse(engine.useGraph(graph, new Uint8Array(), () => true, timestamp));
    const digest = pointer.sha256.replace(/^sha256:/, '');
    const key = new URL(`.quipu-pointer-cache/${digest}`, globalThis.location.href).href;
    const cached = cache ? await cache.match(key) : null;
    const response = cached ?? await fetcher(pointer.url, { credentials: 'omit', redirect: 'error' });
    const bytes = await boundedBytes(response);
    if (await authorize(pointer) !== true) throw new Error('receiver pointer trust refused');
    const result = JSON.parse(engine.useGraph(graph, bytes, () => true, timestamp));
    if (cache && !cached) await cache.put(key, new Response(bytes));
    return result;
  } catch (error) {
    throw new Error(`pack pointer ${graph}: ${error.message ?? error}`);
  }
}
