"""Assemble <frames-dir>/*.png (named by elapsed ms) into a looping GIF. usage: make-gif.py <frames> <out.gif> [width]"""
import sys
from pathlib import Path
from PIL import Image
frames = sorted(Path(sys.argv[1]).glob('*.png'))
width = int(sys.argv[3]) if len(sys.argv) > 3 else 960
times = [int(f.stem) for f in frames]
imgs = []
for f in frames:
    im = Image.open(f).convert('RGB')
    im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    imgs.append(im.quantize(colors=96, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
durations = [max(20, b - a) for a, b in zip(times, times[1:])] + [3000]
imgs[0].save(sys.argv[2], save_all=True, append_images=imgs[1:], duration=durations, loop=0, optimize=True)
print(len(imgs), 'frames,', sum(durations) / 1000, 's')
