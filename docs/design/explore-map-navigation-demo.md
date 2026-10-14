# Explore Map navigation demonstration

[Watch or download the 31-second WebM recording](../../assets/explore-map-navigation.webm).
The small recording is retained with the other repository assets. Open the file
in a browser if the forge displays a download link instead of a player.

The first half selects **Asserted types preserve graph boundaries** on the
previous prototype: its card has no queried property or provenance disclosures,
and selecting it leaves the URL unchanged. The second half selects the same
entity, opens its properties, follows three labelled related-entity links,
and uses Back. Watch the card and the selected node at the center of the map.

Before source: `28f56dfa647207973a2af131a38878be6fc59f2d`.
After source: `bb6ba73892d40b9408ea5ca8a761389c1184ad86`.
Both use the same local WASM engine (released 0.11.1, engine commit
`27699d22d3dc26e2c3d901c731612876f8b21364`) and 88,288-triple receiver pack.
This demonstrates the prototype; it does not establish deployment or final
full-feature engine acceptance. Later pack-selection and publication checks
preserve the recorded navigation behavior.

With the browser engine and a repository pack staged by `just explorer`, and
Playwright installed, reproduce the acceptance with:

```sh
just explorer-navigation --headed
```

The acceptance compares each related list with independent SPARQL queries,
checks all three hops and camera centers, walks Back and Forward, reloads a deep
link, follows an entity outside the initial overview, and tests a late worker
reply, invalid IRI, and a pack without the vision. It also checks the fact drawer,
390px layout and browser errors. The recording illustrates these interactions;
the assertions establish relation correctness.
