// Receiving provenance distinguishes producer validation from checks in this tab.
const $ = selector => document.querySelector(selector);
const el = (tag, attributes = {}) => {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attributes)) {
    if (key === "text") node.textContent = value;
    else node.setAttribute(key, value);
  }
  return node;
};
export function renderProvenance(report, source, timings, { fmt, short }) {
  const build = window.__quipuBuild ?? { version: "?", git_sha: "?" };
  const m = report.manifest;
  const imp = report.import;
  const facts = [
    ["Producer", `quipu ${m.producer.version}`],
    ["Explorer build", `quipu ${build.version} (${String(build.git_sha).slice(0, 12)})`],
    ["Pack built", m.created_at],
    ["Graph hash", m.graph_hash],
    ["Share id", m.share_id],
    ["Canonicalization", m.canonicalization ?? "(none declared)"],
    ["Tx anchor", String(m.tx_anchor)],
    ["Source", source],
    ["Validation location", report.receiving?.mode === "producer-validated" ? "Published pack build" : "This tab"],
    ["Import outcome", imp.outcome],
    ["Triples accepted", fmt(imp.triples.accepted)],
    ["SHACL conforms", String(imp.validation.conforms)],
    ["SHACL warnings", fmt(imp.validation.report.warnings ?? 0)],
    ["Quarantined", fmt(imp.triples.quarantined)],
    ["Off-vocabulary types", imp.validation.off_vocabulary.length
      ? imp.validation.off_vocabulary.map(short).join(", ")
      : "none"],
    ["Promotion", report.promotion
      ? `${report.promotion.outcome} (tx ${report.promotion.tx_id}, ${fmt(report.promotion.triples)} triples)`
      : `refused — ${imp.promotion.blockers.join(", ") || "not eligible"}`],
    ["Load time", `${(timings.load / 1000).toFixed(1)} s in this tab`],
  ];
  const dl = $("#provenance");
  dl.replaceChildren();
  for (const [k, v] of facts) {
    dl.append(el("dt", { text: k }), el("dd", { class: "mono", text: v }));
  }

  // Say what this build did NOT check, rather than letting `conforms: true`
  // read as a clean bill of health it did not earn.
  $("#shacl-note").textContent = report.receiving?.mode === "producer-validated"
    ? "This pack was validated when published; findings are retained above. This tab checked the exact published bytes. Validate pack runs the full checks again here."
    : report.shacl_compiled
    ? "SHACL is compiled into this build, and the report above is a real validation result."
    : "SHACL is NOT compiled into this browser build, so “conforms” above is a default, not a "
      + "finding. The vocabulary check (off-vocabulary types) DID run — it does not need the "
      + "SHACL engine. Run `quipu import` locally for the full validation.";
}
