# Финальный лист 35 пони (roster.json): официальные — из official_render/,
# фанатские — символ "3Q ► ALL" из своей .fla -> official_render/<имя>.png.
# Итог: G:/mlp_2d/roster_35.png
import io
import json
import os
import subprocess

import resvg_py
from PIL import Image, ImageDraw, ImageFont

ROOT = "G:/mlp_2d"
PY = os.path.join(ROOT, "tools", "venv", "Scripts", "python.exe")
OUT = os.path.join(ROOT, "official_render")
FAN = {
    "blossomforth": ("xfl/fan_Blossom_Forth", "BlossomForth 3Q Puppet Parts/BF3Q ► ALL"),
    "cloudchaser": ("xfl/fan_Cloudchaser", "Cloudchaser Puppet Parts/Cloudchaser 3Q Puppet Parts/ CC3Q ► ALL"),
    "flitter": ("xfl/fan_Flitter", "Flitter 3Q Puppet Parts/ TM3Q ► ALL"),
    "sunset_shimmer": ("xfl/fan_Sunset_Shimmer", "Twilight 3Q Puppet Parts/TS3Q ► ALL"),
}
# Глубокая вложенность символов у фанатских файлов — больше стек и лимит рекурсии.
RUN = ("import sys,threading,runpy\n"
       "sys.setrecursionlimit(100000); threading.stack_size(256*1024*1024-4096)\n"
       "def go():\n"
       "    sys.argv=['xfl2svg']+sys.argv[1:]\n"
       "    try: runpy.run_module('xfl2svg',run_name='__main__')\n"
       "    except SystemExit: pass\n"
       "t=threading.Thread(target=go); t.start(); t.join()\n")
for name, (xfl, sym) in FAN.items():
    d = os.path.join(OUT, "_svg", name)
    os.makedirs(d, exist_ok=True)
    for f in os.listdir(d):
        os.remove(os.path.join(d, f))
    subprocess.run([PY, "-X", "utf8", "-c", RUN, os.path.join(ROOT, xfl), sym, d, "--last-frame", "1", "--center",
                    "--no-background", "--timeline-type", "symbol", "--width", "900", "--height", "900"],
                   capture_output=True, text=True, env={**os.environ, "PYTHONUTF8": "1"})
    svgs = [f for f in os.listdir(d) if f.endswith(".svg")]
    if not svgs:
        print("нет", name)
        continue
    im = Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(
        svg_string=open(os.path.join(d, svgs[0]), encoding="utf-8").read(), width=3200)))).convert("RGBA")
    bb = im.getbbox()
    if bb:
        im = im.crop(bb)
    im.save(os.path.join(OUT, name + ".png"))
    print("ok", name)

roster = json.load(open(os.path.join(ROOT, "roster.json"), encoding="utf-8"))
names = sorted(roster, key=lambda k: (roster[k]["source"] != "official", k))
MAIN = ["twilight_sparkle", "applejack", "rainbow_dash", "pinkie_pie", "rarity", "fluttershy", "princess_celestia", "princess_luna"]
names = MAIN + sorted(k for k in roster if k not in MAIN)
T, C = 270, 7
font = ImageFont.truetype("arial.ttf", 17)
rows = (len(names) + C - 1) // C
sheet = Image.new("RGB", (C * T, rows * (T + 24) + 10), (255, 255, 255))
dr = ImageDraw.Draw(sheet)
for i, n in enumerate(names):
    p = os.path.join(OUT, n + ".png")
    x, y = (i % C) * T, (i // C) * (T + 24)
    label = n + ("  (фан)" if roster[n]["source"] == "fan" else "")
    dr.text((x + 6, y + 3), label, fill=(0, 0, 0) if roster[n]["source"] == "official" else (200, 0, 0), font=font)
    if not os.path.exists(p):
        continue
    im = Image.open(p).convert("RGBA")
    im.thumbnail((T - 14, T - 14))
    sheet.paste(im, (x + (T - im.width) // 2, y + 24 + (T - im.height) // 2), im)
sheet.save(os.path.join(ROOT, "roster_35.png"))
print("лист", len(names), sheet.size)
