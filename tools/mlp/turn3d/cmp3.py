import sys, glob
from PIL import Image, ImageDraw
d, out, labs = sys.argv[1], sys.argv[2], sys.argv[3].split(",")
extra = sys.argv[4:]
fs = [sorted(glob.glob(f"{d}/{x}/*.png"))[0] for x in "anb"] + extra
S = 420; sh = Image.new("RGB", (S * len(fs), S + 24), "white"); dr = ImageDraw.Draw(sh)
for i, (p, l) in enumerate(zip(fs, labs)):
    im = Image.open(p).convert("RGBA"); im = im.crop(im.getbbox()); k = (S - 10) / max(im.size); im = im.resize((int(im.width * k), int(im.height * k)))
    bg = Image.new("RGBA", (S, S), __import__("os").environ.get("BG", "white")); bg.alpha_composite(im, ((S - im.width) // 2, S - im.height)); sh.paste(bg.convert("RGB"), (i * S, 24)); dr.text((i * S + 8, 4), l, fill="black")
sh.save(out)
