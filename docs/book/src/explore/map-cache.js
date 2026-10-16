// A saved visual preview is never an imported store or write authority.
// The native worker still receives and verifies the complete pack on every load.
const SLOT = "quipu:map-preview:v1";
const LIMIT = 2_000_000;
const validRows = (result, fields) => Array.isArray(result?.rows) && result.rows.length <= 2000
  && result.rows.every((row) => fields.every((field) => typeof row?.[field] === "string" && row[field].length <= 16384));
const valid = (snapshot) => validRows(snapshot?.knowledge, ["subject", "predicate", "object"])
  && validRows(snapshot?.files, ["subject", "kind", "path"]);

export async function mapCache(bytes, build) {
  try {
    if (!build?.git_sha || !build?.version) return null;
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const hash = [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
    const identity = JSON.stringify({ hash, build });
    const storage = window.localStorage;
    return {
      read() {
        try {
          const text = storage.getItem(SLOT);
          if (!text || text.length > LIMIT) return null;
          const saved = JSON.parse(text);
          return saved.identity === identity && valid(saved.snapshot) ? saved.snapshot : null;
        } catch { return null; }
      },
      write(snapshot) {
        if (!valid(snapshot)) return;
        const text = JSON.stringify({ identity, snapshot });
        try { if (text.length <= LIMIT) storage.setItem(SLOT, text); } catch { /* Optional cache. */ }
      },
      remove() { try { storage.removeItem(SLOT); } catch { /* Optional cache. */ } },
    };
  } catch { return null; } // Storage restrictions and unavailable digest are a cold load.
}

export function lockPreviewControls() {
  const previous = [];
  for (const control of document.querySelectorAll("button,input,textarea,select,a")) {
    if (control.id === "tab-map" || (control.closest("#view-map") && control.id !== "graph-showq")) continue;
    if (control.tagName === "A") {
      const inert = control.inert;
      previous.push(() => { control.inert = inert; });
      control.inert = true;
    } else {
      const disabled = control.disabled;
      previous.push(() => { control.disabled = disabled; });
      control.disabled = true;
    }
  }
  document.documentElement.dataset.mapPreview = "true";
  return () => {
    for (const restore of previous) restore();
    delete document.documentElement.dataset.mapPreview;
  };
}
