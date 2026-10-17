// The released Quipu engine owns import verification, promotion, and queries.
import init, { Explorer, explorerVersion } from './pkg/quipu_wasm_explorer.js';

const ready = init();
let explorer;
self.onmessage = async ({ data }) => {
  const { id, cmd, bytes, sparql } = data;
  try {
    await ready;
    let result;
    if (cmd === 'version') result = JSON.parse(explorerVersion());
    else if (cmd === 'load') {
      const load = Explorer.loadPendant ?? Explorer.loadQpack;
      explorer = load.call(Explorer, new Uint8Array(bytes), 'embedded-brief', new Date().toISOString());
      result = JSON.parse(explorer.loadReport());
      if (result.promotion?.outcome !== 'promoted' || result.import.triples.quarantined !== 0) {
        explorer = null;
        throw new Error('The pendant was not promoted without quarantine');
      }
    } else if (cmd === 'query' && explorer) result = JSON.parse(explorer.query(sparql));
    else throw new Error('No verified pendant or unsupported read command');
    self.postMessage({ id, ok: true, result });
  } catch (error) {
    self.postMessage({ id, ok: false, error: `${error.name}: ${error.message}` });
  }
};
