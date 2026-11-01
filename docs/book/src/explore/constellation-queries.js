// One query identity for build-time views and live post-edit rendering.
export const DEFAULT_PREFIXES = `PREFIX aegis: <http://aegis.gastown.local/ontology/>
PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
`;
export function constellationQueries(prefixes = DEFAULT_PREFIXES) {
  const K = "https://quipu.dev/knowledge/";
  return { knowledge: `${prefixes}
SELECT ?subject ?predicate ?object WHERE {
  ?s ?p ?o . FILTER(STRSTARTS(STR(?s), "${K}"))
  BIND(STR(?s) AS ?subject) BIND(STR(?p) AS ?predicate) BIND(STR(?o) AS ?object)
} LIMIT 2000`, files: `${prefixes} SELECT ?subject ?kind ?path WHERE {
        ?s rdf:type ?type ; aegis:filePath ?path .
        FILTER(?type = aegis:CodeModule || ?type = aegis:Document)
        BIND(STR(?s) AS ?subject) BIND(STR(?type) AS ?kind)
      } ORDER BY ?path LIMIT 2000` };
}
