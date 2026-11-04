// arming: ci — the shared explorer acceptance action invokes the real bundle.
// Ephemeral test keys stay in node:crypto; only public identity and signatures
// cross the Wasm boundary. No store, network, registration or master write.
import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { createRequire } from "node:module";
import { resolve, join } from "node:path";

const bundle = process.argv[2];
assert.ok(bundle, "usage: node scripts/smoke-wasm-client-changeset.mjs <node-bundle-dir>");
const wasm = createRequire(import.meta.url)(join(resolve(bundle), "quipu_wasm_explorer.js"));
const hash = value => `sha256:${createHash("sha256").update(value).digest("hex")}`;
const base = {
  store_id: "urn:store:synthetic-client", log_epoch: "urn:epoch:synthetic-client",
  base_tx: 3, graph: "ROOT", shapes_hash: hash("synthetic shapes"),
  rows: [{ triple: '<urn:s> <urn:p> "base"', valid_from: "2026-01-01T00:00:00Z", valid_to: null }],
};
const opened = wasm.openClientOverlay(JSON.stringify(base), "urn:fork:synthetic-client");
const overlay = wasm.replaceClientOverlay(opened, JSON.stringify(['<urn:s> <urn:p> "local"']));
assert.deepEqual(JSON.parse(overlay).base, JSON.parse(opened).base, "a local edit must retain its frozen journal");
const identity = {
  key_id: "urn:registration:synthetic", signer: "synthetic-client",
  operation_id: "00000000-0000-4000-8000-000000000001",
  issued_at: "2026-01-02T00:00:00Z", nonce: "ab".repeat(16),
};
const prepared = JSON.parse(wasm.prepareClientChangeset(overlay, JSON.stringify(identity)));
assert.equal(prepared.envelope.scope_digest, hash('{"context_graphs":[],"scope":["ROOT"]}'));
assert.ok(prepared.message.startsWith("quipu-changeset-v1\nrole=client\n"));
assert.ok(prepared.message.includes("fork_id=urn:fork:synthetic-client\n"));
const { privateKey, publicKey } = generateKeyPairSync("ed25519");
const publicHex = publicKey.export({ type: "spki", format: "der" }).subarray(-32).toString("hex");
const proposal = { envelope: prepared.envelope, body: prepared.body,
  signature: sign(null, Buffer.from(prepared.message), privateKey).toString("hex") };
assert.equal(wasm.verifyClientChangesetSignature(JSON.stringify(proposal), publicHex), true);
const refuses = value => assert.throws(() => wasm.verifyClientChangesetSignature(JSON.stringify(value), publicHex));
refuses({ ...proposal, signature: "00".repeat(64) });
refuses({ ...proposal, envelope: { ...proposal.envelope, fork_id: "urn:fork:other" } });
refuses({ ...proposal, envelope: { ...proposal.envelope, audience: "urn:store:other" } });
const changedBody = proposal.body.replace("client-overlay", "other-source");
refuses({ ...proposal, body: changedBody, envelope: { ...proposal.envelope, body_sha256: hash(changedBody) } });
refuses({ ...proposal, signature: sign(null, Buffer.from("v1|another-domain"), privateKey).toString("hex") });
assert.throws(() => wasm.replaceClientOverlay(overlay, JSON.stringify(['_:blank <urn:p> "no"'])));
assert.throws(() => wasm.prepareClientChangeset(overlay, JSON.stringify({ ...identity, signer: "bad\nrole=master" })));
console.log("PASS: compiled Wasm overlay, frozen journal, shared canonical bytes, signature and refusal controls");
