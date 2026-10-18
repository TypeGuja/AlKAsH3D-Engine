# Ряд кадров из папок рендера: python rowsheet.py out.png dir1 [dir2 ...] [--crop 0.5] — каждая папка = ряд.
import glob, sys
from PIL import Image, ImageDraw
args = sys.argv[1:]; crop = 0.0
if "--crop" in args: i = args.index("--crop"); crop = float(args[i + 1]); del args[i:i + 2]
out, dirs = args[0], args[1:]
rows = []
for d in dirs:
    r = []
    for p in sorted(glob.glob(d + "/*.png"))[:9]:
        im = Image.open(p).convert("RGBA"); bg = Image.new("RGBA", im.size, __import__("os").environ.get("BG", "white")); bg.alpha_composite(im)
        im = bg.convert("RGB"); w, h = im.size; r.append(im.crop((0, int(h * crop), w, h)))
    rows.append(r)
w = max(i.width for r in rows for i in r); h = max(i.height for r in rows for i in r); k = min(1.0, 2600 / (w * 9))
W, H = int(w * k), int(h * k)
sh = Image.new("RGB", (W * 9, (H + 16) * len(rows)), "white"); dr = ImageDraw.Draw(sh)
for y, r in enumerate(rows):
    for x, im in enumerate(r):
        sh.paste(im.resize((int(im.width * k), int(im.height * k))), (x * W, y * (H + 16) + 16)); dr.text((x * W + 3, y * (H + 16) + 2), f"{x * 5.625:.1f}", fill="red")
sh.save(out); print(out, sh.size)
