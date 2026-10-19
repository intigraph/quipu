# Cord explorer demonstration

[Watch or download the 20-second WebM recording](../../assets/explore-cords.webm).
GitHub may require downloading the file instead of playing it inline.

Select the vision’s **guides** relation with Enter, expand its history, then
navigate from the subject and inspect the mobile cord overview. Before, edges
were inert and nodes used fixed radii. The candidate exposes relation endpoints,
receiver fact history and degree-sized knots on topic/type pendant cords.

The recording retains the actual browser interaction and timing; its initial
cold loading interval is trimmed. Temporary assert/retract controls operate
inside this tab’s receiving store. Nothing is sent to a server or published.
It is a source-candidate demonstration, not installed acceptance.

## Revisions and reproduction

The public before probe ran against the published explorer on October 19, 2026:
engine `0.14.1`, build `e22dd51bfc2631bf141ed4e7eaa76803147eae7e`.
The source baseline is `a7510ca8ea23605b395f164b8bd56dd3de7c26ac`; the candidate
code is `6c684df2911beb2ffe598152634b6c123ffe98f9`. The recording and screenshots
use that candidate’s code before its commit; the accompanying evidence commit
changes media and this caption. The graph is the ordinary released repository
pendant, verified and imported by the real browser engine.

Run `just explorer release` to stage the released repository pendant, then
`just explorer local` to replace the engine bundle with the current source build.
With Playwright installed, run `just explorer-navigation --headed`.
`CHROMIUM_PATH` can select a system browser.
The focused `scripts/test-explore-cords.cjs` records video and checks the exact
relation history against the engine, including a retained local retraction.
`OUT_DIR` selects its screenshot/video directory. The retained demonstration
adds pauses to make those interactions readable.

The API displays ROOT history in the receiving copy, capped at 2,000 facts.
An RDF share does not recreate original producer transaction history. Original
subject episode/source properties are shown separately and never relabeled as
individual-edge provenance. Older bundles explicitly report history unavailable.
There is no dependency on the reverted labeled-community API: cords use the
existing topic/type groups; degree measures distinct links in the current view.

## Screenshots

[Before: published map](../book/src/images/explore-cords-before.png) ·
[After: relation details](../book/src/images/explore-cords-details.png) ·
[After: 375px cord overview](../book/src/images/explore-cords-mobile.png).

The 375px screenshot illustrates the responsive overview; it is a still image.
The source and embed theme remain the same page and receiving pipeline.
