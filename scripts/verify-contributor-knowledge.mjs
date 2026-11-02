// Run on the RECEIVER after import and promotion, not on producer input.
import { execFileSync } from "node:child_process";
const [binary, db] = process.argv.slice(2);
const ask = (query) => {
  // The native query CLI prints TSV, not the REST/wasm JSON result. These
  // projections contain only IRIs and counts, never multiline prose.
  const text = execFileSync(binary, ["query", query, "--db", db], { encoding: "utf8" }).trim();
  const lines = text.split("\n"), footer = lines.pop().match(/^(\d+) results$/);
  if (!footer || !/^-+$/.test(lines[1])) throw new Error("unrecognized query output");
  const columns = lines[0].split("\t");
  const rows = lines.slice(2).filter(Boolean).map((line) => {
    const values = line.split("\t");
    if (values.length !== columns.length) throw new Error("invalid query row");
    return Object.fromEntries(columns.map((key, i) => [key, values[i]]));
  });
  if (rows.length !== Number(footer[1])) throw new Error("query row count mismatch");
  return rows;
};
// K is the contributor vocabulary (classes and predicates). The ideas themselves
// are published under their w3id names, each linked to its former name.
const K = "https://quipu.dev/knowledge/";
const ID = "https://w3id.org/intigraph/quipu/id/idea/";
const types = ask(`SELECT ?type (COUNT(?s) AS ?n) WHERE { ?s a ?type . FILTER(STRSTARTS(STR(?s), "${ID}")) } GROUP BY ?type`);
for (const suffix of ["Vision", "Episode", "DecisionRecord", "Directive", "Book", "Chapter"]) {
  if (!types.some((r) => r.type.endsWith(suffix) && Number(r.n) > 0)) throw new Error(`missing ${suffix}`);
}
const paths = ask(`SELECT ?decision ?module WHERE { <${ID}vision> <${K}guides> ?decision . ?decision <${K}governs> ?module . ?module a ?type }`);
if (new Set(paths.map((r) => r.decision)).size !== 4) throw new Error("vision must reach four decisions and real typed code witnesses");
const episodes = ask(`SELECT ?episode ?source WHERE { ?episode a <${K}Episode> ; <http://www.w3.org/ns/prov#wasDerivedFrom> ?source }`);
if (episodes.length !== 4 || episodes.some((r) => !r.source.startsWith("https://github.com/scbrown/quipu/blob/main/"))) {
  throw new Error("four source-backed episodes required");
}
const stale = ask(`SELECT (COUNT(DISTINCT ?s) AS ?n) WHERE { ?s ?p ?o . FILTER(STRSTARTS(STR(?s), "${K}") && !STRSTARTS(STR(?s), "${K}publication-policy/")) }`);
if (Number(stale[0].n) !== 0) throw new Error("contributor ideas still published under their former names");
const unlinked = ask(`SELECT (COUNT(DISTINCT ?s) AS ?n) WHERE { ?s ?p ?o . FILTER(STRSTARTS(STR(?s), "${ID}")) FILTER NOT EXISTS { ?s <http://www.w3.org/2002/07/owl#sameAs> ?former . FILTER(STR(?former) = CONCAT("${K}", STRAFTER(STR(?s), "${ID}"))) } }`);
if (Number(unlinked[0].n) !== 0) throw new Error("every published idea must link to its former name");
console.log(`Contributor receiver proof: ${paths.length} vision→decision→typed code paths, ${episodes.length} source-backed episodes`);
