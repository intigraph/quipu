# Shared stack hero: the Thread Workshop

The Quipu hero belongs to the common family used by the Quipu-stack READMEs.
Use the [stack README template](https://github.com/intigraph/tambo/blob/main/docs/stack/README-TEMPLATE.md)
for section order. Keep the same frame, palette, lettering and layout when adding a tool.
Each tool has its own hand-authored object; Quipu uses hanging knotted cords.

The source canvas is 352 × 176 pixels. `header.png` is the integer-scale-4
1408 × 704 render, displayed at 704 CSS pixels. Its transparent margin and warm
rim work on light and dark backgrounds; lettering stays on the opaque ink panel.
Do not smooth, trace, add a series number or encode the stack size in a card.

Palette: ink `#172B3B`, depth `#10212F`, paper `#F2E6CF`, muted `#95AFB4`,
grid `#294554`, teal `#68D4BF`, gold `#EDB96E` and coral `#E78E7D`.
The 5×7 lettering and all geometry are inside `quipu-hero.pxl`; no font or image download is needed.

With pixelsrc's `pxl` built from revision `48023226cacf81ce5ba5e493cc98d94fea5d7e23`:

```sh
pxl validate --strict assets/quipu-hero.pxl
pxl render --strict assets/quipu-hero.pxl --sprite quipu --scale 4 -o assets/header.png
```

The source and rendered bytes are bound in [readme-hero.json](readme-hero.json).
The terminal recording uses the separate shared [terminal harness](../docs/demos/terminal.md).
