// Actual HTTP response boundaries, with an explicitly synthetic local engine.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { BrowserFederatedProvider, BrowserRemoteProvider } from "../wasm/explorer/runtime/browser-runtime.mjs";

const valid = { variables: ["x"], rows: [{ x: "positive" }] };
const malformed = [null, true, 1, "invalid", [], ["invalid"], {},
  { variables: [], rows: null }, { variables: [1], rows: [] },
  { variables: [], rows: [null] }, { variables: [], rows: [[]] }];
let payload = valid;
const server = createServer((request, response) => {
  response.writeHead(200, { "Content-Type": "application/json" });
  response.end(JSON.stringify(payload));
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const explorer = (value) => ({
  remoteLabel: () => "{}", checkFederation() {}, query: () => JSON.stringify(value),
});
const remote = new BrowserRemoteProvider(explorer(valid), {
  name: "peer", endpoint: `http://127.0.0.1:${server.address().port}`,
});
try {
  for (const value of malformed) {
    payload = value;
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      await assert.rejects(remote.query("SELECT ?x WHERE {?x ?p ?o}"), /response must be an object/);
    }
    const result = await new BrowserFederatedProvider(explorer(valid), [remote]).query("SELECT ?x WHERE {?x ?p ?o}");
    assert.equal(result.complete, false);
    assert.equal(result.result.rows.length, 1);
    assert.equal(result.providers[0].ok, true);
    assert.equal(result.providers[1].ok, false);
    assert.equal(result.providers[1].rows, 0);
  }
  payload = valid;
  for (const value of malformed) {
    const result = await new BrowserFederatedProvider(explorer(value), [remote]).query("SELECT ?x WHERE {?x ?p ?o}");
    assert.equal(result.complete, false);
    assert.equal(result.result.rows.length, 1);
    assert.equal(result.providers[0].ok, false);
    assert.equal(result.providers[1].ok, true);
  }
  for (const value of [valid, { variables: ["x"], rows: [] }]) {
    payload = value;
    const result = await new BrowserFederatedProvider(explorer(valid), [remote]).query("SELECT ?x WHERE {?x ?p ?o}");
    assert.equal(result.complete, true);
    assert.equal(result.result.rows.length, 1 + value.rows.length);
    assert.ok(result.providers.every((provider) => provider.ok));
  }
  console.log("PASS 11 malformed remote, 11 malformed local, 2 valid HTTP federation controls");
} finally {
  await new Promise((resolve) => server.close(resolve));
}
