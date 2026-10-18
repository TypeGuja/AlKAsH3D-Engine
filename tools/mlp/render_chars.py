# Все символы "~<код>_Character" из XFL (распакованной .fla) -> SVG + PNG
# и лист с подписями. python render_chars.py <папка_xfl> <папка_вывода>
import io
import os
import re
import subprocess
import sys

import resvg_py
from PIL import Image, ImageDraw, ImageFont

xfl, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
PY = os.path.join(os.path.dirname(os.path.abspath(__file__)), "venv", "Scripts", "python.exe")
syms = subprocess.run([PY, "-m", "xfl2svg", xfl, "x", "x", "--print-symbols"], capture_output=True, text=True).stdout
chars = [s.strip() for s in syms.splitlines() if re.search(r"~[^/]+_Character$", s.strip())]
tiles = []
for sym in chars:
    code = re.search(r"~([^/]+)_Character$", sym).group(1)
    d = os.path.join(out, "_svg", code)
    os.makedirs(d, exist_ok=True)
    r = subprocess.run([PY, "-m", "xfl2svg", xfl, sym, d, "--last-frame", "1", "--center", "--no-background",
                        "--timeline-type", "symbol", "--width", "800", "--height", "800"], capture_output=True, text=True)
    svgs = [f for f in os.listdir(d) if f.endswith(".svg")]
    if not svgs:
        print("нет", code, r.stderr[-300:])
        continue
    svg = open(os.path.join(d, svgs[0]), encoding="utf-8").read()
    im = Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(svg_string=svg, width=400)))).convert("RGBA")
    bb = im.getbbox()
    if bb:
        im = im.crop(bb)
    im.save(os.path.join(out, code + ".png"))
    tiles.append((code, im))
    print("ok", code, flush=True)
T, C = 260, 6
font = ImageFont.truetype("arial.ttf", 20)
rows = (len(tiles) + C - 1) // C
sheet = Image.new("RGB", (C * T, rows * (T + 26)), (255, 255, 255))
dr = ImageDraw.Draw(sheet)
for i, (code, im) in enumerate(tiles):
    im = im.copy()
    im.thumbnail((T - 10, T - 10))
    x, y = (i % C) * T, (i // C) * (T + 26)
    sheet.paste(im, (x + (T - im.width) // 2, y + 26 + (T - im.height) // 2), im)
    dr.text((x + 6, y + 3), code, fill=(200, 0, 0), font=font)
sheet.save(os.path.join(out, "_sheet.png"))
print("лист", sheet.size)
