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
const shapes = '@prefix sh: <http://www.w3.org/ns/shacl#> . [] sh:path <urn:p>; sh:maxCount 1 .';
const base = {
  store_id: "urn:store:synthetic-client", log_epoch: "urn:epoch:synthetic-client",
  base_tx: 3, graph: "ROOT", shapes_hash: hash(shapes),
  rows: [{ triple: '<urn:s> <urn:p> "base"', valid_from: "2026-01-01T00:00:00Z", valid_to: null }],
};
const opened = wasm.openClientOverlay(JSON.stringify(base), "urn:fork:synthetic-client");
const overlay = wasm.replaceClientOverlay(opened, JSON.stringify(['<urn:s> <urn:p> "local"']));
assert.deepEqual(JSON.parse(overlay).base, JSON.parse(opened).base, "a local edit must retain its frozen journal");
const caughtUp = { ...base, base_tx: 4, rows: [...base.rows,
  { ...base.rows[0], triple: '<urn:s> <urn:note> "upstream"' }] };
const preview = JSON.parse(wasm.previewClientOverlay(overlay, JSON.stringify(caughtUp), shapes));
assert.equal(preview.outcome, "composed");
assert.deepEqual(preview.triples, ['<urn:s> <urn:note> "upstream"', '<urn:s> <urn:p> "local"']);
const conflicting = { ...caughtUp, rows: [{ ...base.rows[0], triple: '<urn:s> <urn:p> "master"' }] };
const conflict = JSON.parse(wasm.previewClientOverlay(overlay, JSON.stringify(conflicting), shapes));
assert.equal(conflict.outcome, "conflicts");
assert.equal(conflict.records.length, 1);
assert.equal(Object.hasOwn(conflict, "triples"), false);
assert.throws(() => wasm.previewClientOverlay(overlay, JSON.stringify({ ...caughtUp, log_epoch: "urn:other" }), shapes));
assert.throws(() => wasm.previewClientOverlay(overlay, JSON.stringify(caughtUp), ""));
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
const nextIdentity = { ...identity, operation_id: "00000000-0000-4000-8000-000000000002", nonce: "cd".repeat(16) };
const rebase = (old, current, id = nextIdentity) => JSON.parse(wasm.rebaseClientOverlay(
  overlay, JSON.stringify(old), publicHex, JSON.stringify(current), shapes, JSON.stringify(id)));
const result = rebase(proposal, caughtUp);
assert.equal(result.outcome, "prepared");
const rebased = result.rebased;
assert.deepEqual(rebased.previous, proposal, "old signed operation stays byte-equivalent");
assert.deepEqual(rebased.overlay.base,
  JSON.parse(wasm.openClientOverlay(JSON.stringify(caughtUp), "urn:fork:synthetic-client")).base);
assert.deepEqual(rebased.overlay.proposed, preview.triples);
assert.equal(rebased.supersession.old_operation_id, identity.operation_id);
assert.equal(rebased.supersession.new_operation_id, nextIdentity.operation_id);
assert.equal(rebased.prepared.envelope.base_tx, 4);
assert.equal(rebased.prepared.envelope.delta_sha256, proposal.envelope.delta_sha256);
const nextPrepared = JSON.parse(wasm.prepareClientChangeset(JSON.stringify(rebased.overlay), JSON.stringify(nextIdentity)));
assert.deepEqual(nextPrepared.envelope, rebased.prepared.envelope);
assert.equal(nextPrepared.body, rebased.prepared.body);
assert.equal(rebased.supersession.new_fingerprint, hash(nextPrepared.message));
const nextProposal = { envelope: nextPrepared.envelope, body: nextPrepared.body,
  signature: sign(null, Buffer.from(nextPrepared.message), privateKey).toString("hex") };
assert.equal(wasm.verifyClientChangesetSignature(JSON.stringify(nextProposal), publicHex), true);
assert.throws(() => wasm.verifyClientChangesetSignature(JSON.stringify({ ...nextProposal, signature: proposal.signature }), publicHex));
const rebaseConflict = rebase(proposal, conflicting);
assert.equal(rebaseConflict.outcome, "conflicts");
assert.equal(Object.hasOwn(rebaseConflict, "rebased"), false);
assert.throws(() => rebase(proposal, caughtUp, { ...nextIdentity, operation_id: identity.operation_id }));
assert.throws(() => rebase(proposal, caughtUp, { ...nextIdentity, nonce: identity.nonce }));
assert.throws(() => rebase({ ...proposal, signature: "00".repeat(64) }, caughtUp));
assert.throws(() => rebase(proposal, { ...caughtUp, log_epoch: "urn:other" }));
const changedOverlay = wasm.replaceClientOverlay(overlay, JSON.stringify(['<urn:s> <urn:p> "different"']));
assert.throws(() => wasm.rebaseClientOverlay(changedOverlay, JSON.stringify(proposal), publicHex,
  JSON.stringify(caughtUp), shapes, JSON.stringify(nextIdentity)));
const refuses = value => assert.throws(() => wasm.verifyClientChangesetSignature(JSON.stringify(value), publicHex));
refuses({ ...proposal, signature: "00".repeat(64) });
refuses({ ...proposal, envelope: { ...proposal.envelope, fork_id: "urn:fork:other" } });
refuses({ ...proposal, envelope: { ...proposal.envelope, audience: "urn:store:other" } });
const changedBody = proposal.body.replace("client-overlay", "other-source");
refuses({ ...proposal, body: changedBody, envelope: { ...proposal.envelope, body_sha256: hash(changedBody) } });
refuses({ ...proposal, signature: sign(null, Buffer.from("v1|another-domain"), privateKey).toString("hex") });
assert.throws(() => wasm.replaceClientOverlay(overlay, JSON.stringify(['_:blank <urn:p> "no"'])));
assert.throws(() => wasm.prepareClientChangeset(overlay, JSON.stringify({ ...identity, signer: "bad\nrole=master" })));
console.log("PASS: compiled Wasm overlay, preview, rebase/supersession, canonical bytes, signature and refusal controls");
