import { mapCache, lockPreviewControls } from "./map-cache.js";
import { loadStarted, loaded, setQuery } from "./workbench.js";
const $ = (selector) => document.querySelector(selector);

// The same receiving pipeline serves standalone and embedded presentations.
export function createPackLoader({ ask, constellation, renderProvenance, status, fmt,
  renderTypes, renderBrowser, refreshExport, runSparql, reportReleaseFreshness, initialQuestion }) {
  return async function loadPack(bytes, source) {
    const embed = new URLSearchParams(location.search).get("embed") === "1";
    const cache = embed ? await mapCache(bytes, window.__quipuBuild) : null;
    const saved = cache?.read();
    let unlock = null;
    if (saved) {
      await constellation.load(saved);
      unlock = lockPreviewControls();
      $("main").hidden = false;
    }
    loadStarted(bytes.byteLength);
    status(saved ? "Saved map preview. Verifying a fresh copy before queries and edits…"
      : `Verifying and importing ${fmt(bytes.byteLength)} bytes from ${source}…`);
    const t0 = performance.now();
    let report;
    try {
      report = await ask({ cmd: "load", bytes, source });
    } catch (error) {
      if (saved) { $("main").hidden = true; cache.remove(); }
      unlock?.();
      throw error;
    }
    if (saved && report.promotion?.outcome !== "promoted") {
      $("main").hidden = true;
      cache.remove();
      unlock?.();
      throw new Error("Saved preview discarded: fresh pack was not promoted.");
    }
    unlock?.();
    const load = performance.now() - t0;
    renderProvenance(report, source, { load });
    loaded(report);
    status(`Loaded ${fmt(report.import.triples.accepted)} triples — everything below is a live `
      + `query against this tab's copy.`);
    $("main").hidden = false;
    await renderTypes();
    await constellation.load(null, cache && report.promotion?.outcome === "promoted"
      ? async (snapshot) => {
        const edits = await ask({ cmd: "editLog" });
        if (Array.isArray(edits) && !edits.length) cache.write(snapshot);
      } : null);
    await renderBrowser();
    await refreshExport();
    setQuery(initialQuestion.sparql, initialQuestion.name);
    $("#starter-questions button")?.setAttribute("aria-pressed", "true");
    await runSparql();
    reportReleaseFreshness(report.manifest.producer.version);
  };
}
