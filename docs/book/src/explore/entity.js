// Selected entities use the whole loaded graph, not the overview's subset.
const RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_LABEL = "http://www.w3.org/2000/01/rdf-schema#label";

export function entityIri(value) {
  if (typeof value !== "string" || !/^[a-z][a-z0-9+.-]*:/i.test(value)
      || /[<>"{}|^`\\\s\x00-\x20]/.test(value)) throw new Error("Invalid entity IRI");
  return `<${value}>`;
}

export function entityQueries(id) {
  const iri = entityIri(id);
  return {
    outgoing: `SELECT ?predicate ?value ?isEntity ?label WHERE {
      ${iri} ?p ?o . OPTIONAL { ?o <${RDFS_LABEL}> ?label }
      BIND(STR(?p) AS ?predicate) BIND(STR(?o) AS ?value)
      BIND(isIRI(?o) AS ?isEntity)
    } ORDER BY ?predicate ?value`,
    incoming: `SELECT ?predicate ?value ?label WHERE {
      ?s ?p ${iri} . OPTIONAL { ?s <${RDFS_LABEL}> ?label }
      BIND(STR(?p) AS ?predicate) BIND(STR(?s) AS ?value)
    } ORDER BY ?predicate ?value`,
  };
}

export async function readEntity(query, id) {
  const queries = entityQueries(id);
  const [out, incoming] = await Promise.all([query(queries.outgoing), query(queries.incoming)]);
  if (!Array.isArray(out?.rows) || !Array.isArray(incoming?.rows)) {
    throw new Error("The entity query returned no readable result");
  }
  const distinct = (rows) => [...new Map(rows.map((r) => [
    JSON.stringify([r.predicate, r.value]), r,
  ])).values()];
  const outgoing = distinct(out.rows);
  const isEntity = (r) => r.isEntity === true || r.isEntity === "true";
  return {
    label: outgoing.find((r) => r.predicate === RDFS_LABEL)?.value,
    types: outgoing.filter((r) => r.predicate === RDF_TYPE).map((r) => r.value),
    properties: outgoing.filter((r) => r.predicate !== RDF_TYPE),
    links: [
      ...outgoing.filter(isEntity).map((r) => ({ ...r, direction: "outgoing" })),
      ...distinct(incoming.rows).map((r) => ({ ...r, direction: "incoming" })),
    ],
    truncated: !!(out.truncated || incoming.truncated), queries,
  };
}

export function renderEntity(card, id, entity, { short, navigate, inspect, relation }) {
  card.replaceChildren();
  const text = (parent, tag, value, className) => {
    const e = document.createElement(tag); e.textContent = value;
    if (className) e.className = className;
    parent.append(e); return e;
  };
  text(card, "h3", entity.label || short(id));
  text(card, "p", id, "entity-iri mono");
  text(card, "p", entity.types.length ? entity.types.map(short).join(", ")
    : "No type recorded", "entity-types");
  const description = entity.properties.find((r) => r.predicate === "http://www.w3.org/2000/01/rdf-schema#comment");
  if (description) text(card, "p", description.value);
  const excerpt = entity.properties.find((r) => r.predicate === "https://quipu.dev/knowledge/excerpt");
  if (excerpt) text(card, "blockquote", excerpt.value);
  const source = entity.properties.find((r) => r.predicate === "http://www.w3.org/ns/prov#wasDerivedFrom");
  const file = entity.properties.find((r) => r.predicate.endsWith("/filePath"));
  const sourceUrl = source?.value || (file && `https://github.com/intigraph/quipu/blob/main/${file.value}`);
  if (sourceUrl && /^https:\/\/github\.com\/scbrown\/quipu\/blob\//.test(sourceUrl)) {
    const link = text(card, "a", "Read the source ↗"); link.href = sourceUrl;
    link.target = "_blank"; link.rel = "noopener noreferrer";
  }
  if (entity.truncated) text(card, "p", "The engine truncated this result; related links may be incomplete.", "bad");
  const provenance = (p) => /www\.w3\.org\/ns\/prov#/.test(p)
    || /[/#](source|sourceKind|observedAt|validFrom|validTo|valid_from|valid_to|txTime|transactionTime|wasDerivedFrom|wasGeneratedBy|authored_by)$/.test(p);
  for (const [title, className, include] of [
    ["Properties", "entity-properties", (p) => !provenance(p)],
    ["Provenance", "entity-provenance", provenance],
  ]) {
    const section = document.createElement("details"); section.className = className;
    text(section, "summary", title); const dl = document.createElement("dl");
    for (const r of entity.properties.filter((r) => include(r.predicate))) {
      text(dl, "dt", short(r.predicate)).title = r.predicate;
      text(dl, "dd", r.value);
    }
    section.append(dl);
    if (!dl.children.length) text(section, "p", "None recorded.", "muted");
    card.append(section);
  }
  text(card, "h4", "Related entities");
  const links = document.createElement("div"); links.className = "node-links";
  for (const r of entity.links) {
    const b = text(links, "button", `${r.direction === "outgoing" ? "→" : "←"} ${short(r.predicate).split(/[/#]/).pop()}: ${r.label || short(r.value)}`);
    b.dataset.entity = r.value; b.dataset.predicate = r.predicate;
    b.dataset.direction = r.direction; b.title = `${r.predicate}\n${r.value}`;
    b.onclick = () => navigate(r.value);
    if (relation) {
      const details = text(links, "a", "Relation details"); details.href = "#node-card";
      details.onclick = (event) => { event.preventDefault(); relation(r.direction === "outgoing"
        ? {from:id,to:r.value,rel:r.predicate} : {from:r.value,to:id,rel:r.predicate}); };
    }
  }
  if (!entity.links.length) text(links, "p", "No related entities recorded.", "muted");
  card.append(links);
  text(card, "button", "Inspect facts & edit").onclick = () => inspect({ iri: id, path: entity.label || short(id) });
}
