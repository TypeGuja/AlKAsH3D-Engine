# python pick.py out.png crop idx,idx,... dir1 [dir2...] — выбранные кадры (индексы 0..8) крупно, ряд на папку
import glob, sys
from PIL import Image, ImageDraw
out, crop, idx, dirs = sys.argv[1], float(sys.argv[2]), [int(x) for x in sys.argv[3].split(",")], sys.argv[4:]
rows = []
for d in dirs:
    fs = sorted(glob.glob(d + "/*.png")); r = []
    for i in idx:
        im = Image.open(fs[i]).convert("RGBA"); bg = Image.new("RGBA", im.size, __import__("os").environ.get("BG", "white")); bg.alpha_composite(im)
        w, h = bg.size; r.append(bg.convert("RGB").crop((int(w * 0.25), 0, int(w * 0.8), int(h * crop))))
    rows.append(r)
w = max(i.width for r in rows for i in r); h = max(i.height for r in rows for i in r)
sh = Image.new("RGB", (w * len(idx), (h + 18) * len(rows)), "white"); dr = ImageDraw.Draw(sh)
for y, r in enumerate(rows):
    for x, im in enumerate(r):
        sh.paste(im, (x * w, y * (h + 18) + 18)); dr.text((x * w + 4, y * (h + 18) + 3), f"{dirs[y][-6:]} {idx[x] * 5.625:.1f}", fill="red")
if sh.width > 2400: sh = sh.resize((2400, int(sh.height * 2400 / sh.width)))
sh.save(out); print(out, sh.size)
