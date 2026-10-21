// Replay against two real wasm-bindgen --target nodejs bundle entrypoints.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { resolve } from "node:path";

const require = createRequire(import.meta.url);
const [beforePath, afterPath] = process.argv.slice(2);
if (!beforePath || !afterPath) throw new Error("usage: node demo.mjs <before.js> <after.js>");
const before = require(resolve(beforePath));
const after = require(resolve(afterPath));
assert.equal(typeof before.Explorer, "function");
assert.equal(typeof before.Database, "undefined");
console.log("BEFORE: existing Explorer=function; general Database=undefined");
assert.equal(typeof after.Database, "function");
const database = new after.Database();
const invoke = (operation, input) => database.invoke(operation, JSON.stringify(input));
try {
  const caps = JSON.parse(database.capabilities());
  console.log(`AFTER: Database=function; storage=${caps.storage}; schema=${caps.schema_state}; full_parity=${caps.full_parity}`);
  const input = {turtle: '<urn:good> a <urn:Record>; <urn:title> "Before"; <urn:n> 9007199254740993 .'};
  assert.throws(() => invoke("quipu_knot", input), (error) => {
    const result = JSON.parse(String(error));
    console.log(`write before adoption: ${result.code}; commit_state=${result.commit_state}`);
    return result.code === "schema_not_adopted" && result.commit_state === "not_started";
  });
  invoke("quipu_shapes", {action: "load", name: "demo",
    turtle: "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:Shape> a sh:NodeShape; sh:targetClass <urn:Record>; sh:property [sh:path <urn:title>; sh:minCount 1] ."});
  const admitted = JSON.parse(invoke("quipu_knot", input));
  assert.ok(admitted.tx_id > 0);
  console.log("write after explicit shape adoption: committed");
  let refused = false;
  try { refused = JSON.parse(invoke("quipu_knot", {turtle: "<urn:bad> a <urn:Record> ."})).conforms === false; }
  catch (error) { refused = JSON.parse(String(error)).details?.kind === "validation_failed"; }
  assert.equal(refused, true);
  const rows = JSON.parse(invoke("quipu_query", {query: "SELECT ?s WHERE { ?s a <urn:Record> }"}));
  assert.equal(rows.count, 1);
  console.log(`missing title refused; current Record count=${rows.count} (positive control retained)`);
  const raw = invoke("quipu_entity_history", {iri: "urn:good"});
  assert.match(raw, /9007199254740993/);
  assert.doesNotMatch(raw, /9007199254740992/);
  console.log("history JSON retains exact integer token 9007199254740993");
} finally { database.free(); }
