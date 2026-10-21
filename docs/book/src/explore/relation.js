import { entityIri } from "./entity.js";

const add = (parent, tag, value, className) => {
  const node = document.createElement(tag); node.textContent = value;
  if (className) node.className = className;
  parent.append(node); return node;
};

// History belongs to THIS receiving store. A static RDF share cannot reconstruct
// a producer's transaction log. Never label import time as original author time.
export async function showHistory(card, subject, { factHistory, short, current }, edge = null) {
  const section = document.createElement("details"); section.className = "relation-history";
  add(section, "summary", "Fact history, valid time & transaction time");
  add(section, "p", "Receiver history for this loaded copy. Original producer history is not reconstructed from RDF. Episode/source properties are shown separately.", "muted");
  card.append(section);
  try {
    const result = await factHistory(subject);
    if (!current()) return;
    const facts = result.facts.filter((f) => !edge || (f.predicate === edge.rel
      && f.object.kind === "iri" && f.object.value === edge.to));
    if (!facts.length) add(section, "p", "No matching fact history recorded in this copy.");
    for (const f of facts) {
      const item = document.createElement("dl");
      for (const [key, value] of [
        ["Predicate", f.predicate], ["Object", f.object.kind === "bytes" ? JSON.stringify(f.object.value) : f.object.value],
        ["Operation", f.operation],
        ["Valid from", f.valid_from], ["Valid to", f.valid_to ?? "Open interval"],
        ["Transaction", f.tx], ["Transaction time", f.transaction_time ?? "Not recorded"],
        ["Source", f.source ?? "Not recorded"], ["Actor", f.actor ?? "Not recorded"],
      ]) { add(item, "dt", key); add(item, "dd", String(value)); }
      if (f.object.datatype) { add(item, "dt", "Datatype"); add(item, "dd", f.object.datatype); }
      if (f.object.language) { add(item, "dt", "Language"); add(item, "dd", f.object.language); }
      section.append(item);
    }
    if (result.truncated) add(section, "p", "History truncated at 2,000 facts; this view is incomplete.", "bad");
  } catch (error) {
    if (current()) add(section, "p", `History unavailable: ${error.message}`, "muted");
  }
}

export async function showRelation(card, edge, { query, short, navigate, factHistory, current }) {
  card.replaceChildren(); card.setAttribute("aria-busy", "true");
  add(card, "h3", "Relation details");
  const dl = document.createElement("dl"); card.append(dl);
  for (const [label, iri] of [["Subject", edge.from], ["Predicate", edge.rel], ["Object", edge.to]]) {
    entityIri(iri); add(dl, "dt", label);
    const dd = document.createElement("dd"); dl.append(dd);
    const button = add(dd, "button", short(iri).split(/[/#]/).pop()); button.title = iri;
    button.onclick = () => navigate(iri); add(dd, "p", iri, "entity-iri mono");
  }
  try {
    const result = await query(`SELECT ?predicate ?value WHERE {
      ${entityIri(edge.from)} ?p ?o .
      FILTER(STRSTARTS(STR(?p), "http://www.w3.org/ns/prov#")
        || REGEX(STR(?p), "[/#](source|sourceKind|observedAt|valid_from|valid_to|validFrom|validTo|transactionTime|txTime)$"))
      BIND(STR(?p) AS ?predicate) BIND(STR(?o) AS ?value)
    }`);
    if (!current()) return;
    add(card, "h4", "Subject provenance / episode sources");
    add(card, "p", "These properties describe the subject; they are not assertions of provenance for this individual edge.", "muted");
    const props = document.createElement("dl"); card.append(props);
    for (const row of result.rows) { add(props, "dt", row.predicate); add(props, "dd", row.value); }
    if (!result.rows.length) add(card, "p", "No subject provenance properties recorded.");
    if (result.truncated) add(card, "p", "Provenance result truncated.", "bad");
    await showHistory(card, edge.from, { factHistory, short, current }, edge);
  } catch (error) { if (current()) add(card, "p", `Provenance unavailable: ${error.message}`, "bad"); }
  if (current()) card.setAttribute("aria-busy", "false");
}
