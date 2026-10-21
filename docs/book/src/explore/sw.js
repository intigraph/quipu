// A quipu server on this origin, with no server (aegis-onew9p §4.2).
//
// Answers POSTs to ./query, ./episode, ./set, ./stats, ./delta and ./search by
// relaying into the module worker that hosts the real store, so any in-page
// code — and any agent driving the page — can `fetch("./query", {...})` and get
// the same JSON a quipu HTTP server returns.
//
// ── THE ROUTES ARE SCOPE-RELATIVE, AND THAT IS NOT A STYLE CHOICE ───────────
//
// The design says `fetch('/query')`. On this site that would never be
// intercepted. A Service Worker's scope is the directory it is served from, and
// this one lives beside the page at `/quipu/explore/`. Widening scope to the
// origin root needs a `Service-Worker-Allowed` response header, which GitHub
// Pages does not let anyone set — and `/quipu/` is this project's root anyway,
// so `/query` is not in its space at all.
//
// So the routes are `./query` etc., resolving to `/quipu/explore/query`. An
// absolute `/query` gets a 404 from Pages, which reads as "the service worker
// is broken" rather than "that path was never mine" — the same class of
// misleading signal this page's other refusals are written to avoid. The
// recipe on the page uses the relative form for that reason.
//
// ── WHAT ANSWERS, AND WHAT CANNOT ──────────────────────────────────────────
//
// The store lives in a DEDICATED worker owned by the page, which a Service
// Worker cannot address. So a request is relayed: SW -> a controlled client ->
// that client's module worker -> back down the same MessageChannel. The
// consequence is stated rather than hidden: if no page is open, or the pack has
// not finished loading, there is nothing to ask, and this returns a JSON error
// saying exactly that. It never returns an empty result set, because an empty
// result is indistinguishable from a successful query that matched nothing.

const ROUTES = new Set(["query", "episode", "set", "retract", "stats", "delta", "search", "validate", "ontology"]);

// Immediate control, so the page that registered this worker is served by it
// without a reload. Without both of these the first visit silently gets no
// interception, which looks like a broken worker rather than a pending one.
self.addEventListener("install", (e) => e.waitUntil(self.skipWaiting()));
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });

async function relay(verb, body) {
  // `includeUncontrolled` because a client that has not yet been claimed still
  // holds a perfectly good store — refusing it would be an artifact of timing.
  const clients = await self.clients.matchAll({
    type: "window",
    includeUncontrolled: true,
  });
  if (!clients.length) {
    return json({
      error: "no_client",
      reason:
        "This store lives in a page. No explore page is open, so there is "
        + "nothing to query. Open /quipu/explore/ and retry.",
    }, 503);
  }

  // A bounded wait, because a hung relay must not become a hung fetch. The
  // load is seconds of solid CPU on a 61k-triple pack, so this is generous
  // enough not to fire during a normal load and short enough to be an answer.
  return await new Promise((resolve) => {
    const chan = new MessageChannel();
    const timeoutMs = verb === "search" ? 180_000 : 30_000;
    const timer = setTimeout(
      () =>
        resolve(json({
          error: "timeout",
          reason:
            `The page did not answer within ${timeoutMs / 1000}s. `
            + "It may still be loading the pack or the search model.",
        }, 504)),
      timeoutMs,
    );
    chan.port1.onmessage = (ev) => {
      clearTimeout(timer);
      const r = ev.data;
      resolve(r && r.ok ? json(r.result) : json({
        error: "store_error",
        reason: String(r && r.error ? r.error : "unknown"),
        ...(r?.validation ? { validation: r.validation } : {}),
      }, 500));
    };
    clients[0].postMessage({ quipuVerb: verb, body }, [chan.port2]);
  });
}

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  // Derive the verb from the SCOPE, not from a hardcoded prefix: this worker
  // must keep working if the book's site-url changes, and the scope is the one
  // thing that always describes where it actually lives.
  const scopePath = new URL(self.registration.scope).pathname;
  if (!url.pathname.startsWith(scopePath)) return;
  const verb = url.pathname.slice(scopePath.length);
  if (!ROUTES.has(verb)) return;

  event.respondWith((async () => {
    if (event.request.method !== "POST") {
      return json({
        error: "method_not_allowed",
        reason: `POST a JSON body to ./${verb}.`,
      }, 405);
    }
    let body;
    try {
      body = await event.request.json();
    } catch {
      return json({ error: "bad_request", reason: "body must be JSON" }, 400);
    }
    return await relay(verb, body);
  })());
});
