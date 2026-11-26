# Quipu workshop hero

Original pixel art from the shared Thread Workshop family. The knotted cords
represent connected facts. The transparent margin and warm rim suit light and
dark backgrounds; the opaque panel preserves the lettering contrast.

The source canvas is 352 × 176. Display the standard image at 704 × 352 or
smaller; the double-resolution export is 1408 × 704. No stock art, imported
font, series number or stack-size denominator is used.

Reproduce with pixelsrc commit
`48023226cacf81ce5ba5e493cc98d94fea5d7e23`:

```sh
pxl validate --strict quipu.pxl
pxl render --strict quipu.pxl --sprite quipu --scale 2 -o quipu.png
pxl render --strict quipu.pxl --sprite quipu --scale 4 -o quipu@2x.png
```
