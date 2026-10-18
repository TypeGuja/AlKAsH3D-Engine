# Лист 33 кадров поворота: python turnsheet.py out.png dirA [dirB] [--gif out.gif]
import glob, sys
from PIL import Image, ImageDraw
args = sys.argv[1:]; gif = None
if "--gif" in args: i = args.index("--gif"); gif = args[i + 1]; del args[i:i + 2]
out, dirs = args[0], args[1:]
rows = []
for d in dirs:
    fs = sorted(glob.glob(d + "/*.png"))[:33]; r = []
    for p in fs:
        im = Image.open(p).convert("RGBA"); bg = Image.new("RGBA", im.size, __import__("os").environ.get("BG", "white")); bg.alpha_composite(im); r.append(bg.convert("RGB"))
    rows.append(r)
w, h = rows[0][0].size; S = 170; k = S / max(w, h); W, H = int(w * k), int(h * k)
sh = Image.new("RGB", (W * 11, (H + 14) * 3 * len(rows)), "white"); dr = ImageDraw.Draw(sh)
for ri, r in enumerate(rows):
    for i, im in enumerate(r):
        x, y = (i % 11) * W, (ri * 3 + i // 11) * (H + 14)
        sh.paste(im.resize((W, H)), (x, y + 14)); dr.text((x + 3, y + 1), f"{i * 5.625:.2f}", fill="red")
sh.save(out); print(out, sh.size)
if gif:
    fr = [im.resize((int(w * 0.6), int(h * 0.6))) for im in rows[0]]; fr = fr + fr[-2:0:-1]
    fr[0].save(gif, save_all=True, append_images=fr[1:], duration=70, loop=0); print(gif)
