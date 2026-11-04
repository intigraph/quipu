// Only transferred bytes have a percentage. Engine startup and graph import
// stay indeterminate until their actual work finishes.
const $ = (selector) => document.querySelector(selector);
const steps = ["engine", "pendant", "indexing", "ready"];
const messages = {
  engine: "Starting the engine in your browser…",
  pendant: "Downloading the pendant…",
  indexing: "Verifying the pendant and indexing your graph…",
  ready: "Your knowledge graph is ready.",
};
const bytes = (n) => n < 1024 ? `${n} B` : n < 1048576
  ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;

export function createLoadExperience({ ensureEngine, loadPack, onEngineReady, status }) {
  const panel = $("#graph-loader");
  let busy = false, controlsReady = false, retry = null, pendant = null;
  function stage(state) {
    panel.dataset.state = state;
    document.documentElement.dataset.loadState = state;
    if (new URLSearchParams(location.search).get("embed") === "1" && window.parent !== window) {
      // State only: never disclose a selected file name, URL credentials or graph.
      window.parent.postMessage({ type: "quipu:state", state: state === "ready" || state === "error"
        ? state : "loading", pendant, stage: state }, "*");
    }
    panel.hidden = state === "ready";
    panel.setAttribute("aria-busy", String(state !== "ready" && state !== "error"));
    $("#loading-detail").textContent = messages[state] ?? "";
    if (messages[state] && state !== "ready") status(messages[state]);
    $("#loading-download").hidden = state !== "pendant";
    for (const node of panel.querySelectorAll("[data-stage]")) {
      node.toggleAttribute("data-done", steps.indexOf(node.dataset.stage) < steps.indexOf(state));
      if (node.dataset.stage === state) node.setAttribute("aria-current", "step");
      else node.removeAttribute("aria-current");
    }
  }
  function progress(received, total) {
    const bar = $("#loading-progress");
    if (total > 0) {
      const percent = Math.min(100, Math.floor(received / total * 100));
      bar.value = percent;
      $("#loading-bytes").textContent = `${percent}% · ${bytes(received)} of ${bytes(total)}`;
    } else {
      bar.removeAttribute("value");
      $("#loading-bytes").textContent = `${bytes(received)} downloaded`;
    }
  }
  async function run(read, source, kind = "selected") {
    if (busy) return;
    busy = true;
    pendant = kind;
    retry = () => run(read, source, kind);
    $("main").hidden = true;
    $("#status").classList.remove("bad");
    $("#loading-title").textContent = "Opening your knowledge graph";
    $("#loading-retry").hidden = true;
    for (const node of document.querySelectorAll("#url-load, #file-input, #validate-pack")) node.disabled = true;
    progress(0, 0);
    stage("engine");
    try {
      await ensureEngine();
      if (!controlsReady) { onEngineReady(); controlsReady = true; }
      stage("pendant");
      const data = await read(progress);
      stage("indexing");
      await loadPack(data, source, { defaultPack: pendant === "repository" });
      stage("ready");
    } catch (error) {
      const failedAt = panel.dataset.state;
      stage("error");
      $("main").hidden = true;
      $("#loading-title").textContent = "Could not open the graph";
      const context = failedAt === "engine" ? "The engine could not start."
        : failedAt === "pendant" ? "The pendant could not be downloaded."
        : "The pendant could not be verified or indexed.";
      $("#loading-detail").textContent = `${context} ${error.message || error}`;
      $("#loading-retry").hidden = false;
      status(`Could not load ${source}: ${error.message || error}`);
      $("#status").classList.add("bad");
    } finally {
      busy = false;
      for (const node of document.querySelectorAll("#url-load, #file-input, #validate-pack")) node.disabled = !controlsReady;
    }
  }
  $("#loading-retry").addEventListener("click", () => retry?.());
  return {
    loadUrl: (url, source = url, { defaultPack = false } = {}) => run(async (report) => {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Pendant download failed (HTTP ${response.status}). Check the URL or connection and try again.`);
      const encoding = response.headers.get("content-encoding");
      const length = Number(response.headers.get("content-length"));
      // CORS can hide Content-Encoding while exposing compressed Content-Length.
      // In that case transferred body bytes cannot certify a percentage.
      const total = response.type !== "cors" && (!encoding || encoding === "identity")
        && Number.isFinite(length) ? length : 0;
      report(0, total);
      if (!response.body) {
        const data = await response.arrayBuffer();
        report(data.byteLength, data.byteLength);
        return data;
      }
      const reader = response.body.getReader(), chunks = [];
      let received = 0;
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
        received += value.byteLength;
        report(received, total);
      }
      const data = new Uint8Array(received);
      let offset = 0;
      for (const chunk of chunks) { data.set(chunk, offset); offset += chunk.byteLength; }
      return data.buffer;
    }, source, defaultPack ? "repository" : "selected"),
    loadFile: (file) => run(async (report) => {
      const data = await file.arrayBuffer();
      report(data.byteLength, file.size);
      return data;
    }, file.name),
  };
}
