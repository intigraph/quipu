# Intigraph embed demonstration

- [Before: URL flags ignored, Ask and full chrome (7.76 seconds)](../../assets/explore-embed/before.webm)
- [After: labelled saved Map preview (2.8 seconds)](../../assets/explore-embed/after.webm)
- [After verification: Ask and real bad-write refusal (8 seconds)](../../assets/explore-embed/live.webm)

All clips use `?embed=1&theme=intigraph`, a 1200 × 860 viewport, and the same
released 0.14.1 engine (`e22dd51bfc2631bf141ed4e7eaa76803147eae7e`) and
108,750-triple repository pack. Pack SHA256:
`16a30708eb683173a58b0027c44055a0a9ed1c1867c7a1b232c88897d8bf1752`.
The small WebM files are retained with the repository assets. Browser playback
was checked for all three; download them if the forge does not embed WebM.

Before UI source: `91aae714475b4eed4be43b91da0930c5a89accc2`; all nine served
HTML/JS/CSS assets matched that base byte for byte. After code:
`56766d21147ecc6406d13f15e8e78b2425713e6c`, running locally with staged release
artifacts. The base refresh `8fbe001220f045d1c551fbafeb97702ea67b0388` leaves
those explore assets byte-identical. This is candidate behavior, not deployment.

Watch the Map appear above the fold in Intigraph colors. The saved view says
**Saved map preview** and disables Ask and writes while fresh verification runs.
The third clip shows the real engine after verification, including a refused
write with a populated existing-graph control. Initialization waits are trimmed
from the clips; the untrimmed probes provide timing evidence.

The same visibility probe measured warm Map time **16,827 ms → 196 ms**.
Broader acceptance measured **90 ms** visibility and **15,315 ms** fresh interactive
readiness. The recorded run measured 67 ms and 19,551 ms respectively. These
measurements distinguish a saved visualization from a verified writable store;
the native receiving ceremony still runs on every visit. Storage restrictions,
changed packs or engines, and invalid cache entries take the cold path.

With Playwright available and release artifacts staged by `just explorer release`:

```sh
just explorer-presentation --require-speed
just explorer-navigation --headed
```

The presentation acceptance covers both palettes, normal-page controls, 860px
layout, mobile order and overflow, iframe resizing, cache identity and storage
failures, verification refusal, real Ask rows and the native bad-write control.
The navigation acceptance compares relations with independent SPARQL queries,
including history, deep links, late replies, another pack and invalid IRIs.
Future release artifacts may change the data and timing.
