// Page and browser acceptance share this API over the real worker RPC.
export function createEngineApi(ask) {
  const json = (value) => typeof value === "string" ? value : JSON.stringify(value);
  return {
    query: (sparql) => ask({ cmd: "query", sparql }),
    factHistory: (entity) => ask({ cmd: "factHistory", entity }),
    stats: () => ask({ cmd: "stats" }),
    version: () => ask({ cmd: "version" }),
    capabilities: () => ask({ cmd: "capabilities" }),
    search: (query, limit = 10) => ask({ cmd: "search", query, limit }),
    validatePack: () => ask({ cmd: "validatePack" }),
    validate: (shapes, data) => ask({ cmd: "validate", shapes, data }),
    ontology: (input) => ask({ cmd: "ontology", input: json(input) }),
    remote: (options, sparql, graph) => ask({ cmd: "remote", options, sparql, graph }),
    federate: (remotes, sparql, floor = {}) => ask({ cmd: "federate", remotes, sparql, floor }),
    episode: (body) => ask({ cmd: "episode", episode: json(body) }),
    set: (entity, predicate, value) => ask({ cmd: "set", entity, predicate, value: json(value) }),
    retract: (entity, predicate = "", value = "") => ask({ cmd: "retract", entity, predicate,
      value: value === "" ? "" : json(value) }),
    delta: () => ask({ cmd: "delta" }),
    share: () => ask({ cmd: "exportPack" }),
  };
}
