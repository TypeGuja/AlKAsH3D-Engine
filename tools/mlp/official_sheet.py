# Лист наших пони из официальных сборок DHX: (папка XFL, символ) -> PNG
# в G:/mlp_2d/official_render/<имя>.png и лист _official.png с подписями.
import io
import json
import os
import subprocess

import resvg_py
from PIL import Image, ImageDraw, ImageFont

ROOT = "G:/mlp_2d"
PY = os.path.join(ROOT, "tools", "venv", "Scripts", "python.exe")
F, U, M = "xfl/female", "xfl/unicorns", "xfl/main_"
MAP = {
    "twilight_sparkle": (M + "stk_c01_twilight_sparkle", "~TS_Character"),
    "applejack": (M + "stk_c04_applejack", "~AJ_Character"),
    "rainbow_dash": (M + "stk_c03_rainbow_dash", "~RD_Character"),
    "pinkie_pie": (M + "stk_c02_pinkie_pie", "~PP_Character"),
    "rarity": (M + "stk_c05_rarity", "~RY_Character"),
    "fluttershy": (M + "stk_c06_fluttershy", "~FS_Character"),
    "princess_celestia": (M + "stk_c08_princess_celestia", "~PC_Character"),
    "princess_luna": (M + "STK_C28_Princess_Luna", "~PL_Character"),
    "aloe": (F, "~Aloe_Character"),
    "amethyst_star": (U, "~IUC01_Character"),
    "berry_punch": (F, "BS/~BS_Character"),
    "bon_bon": (F, "BB/~BB_Character"),
    "carrot_top": (F, "GH/~GH_Character"),
    "cheerilee": (F, "~CL_Character"),
    "cherry_berry": (F, "IP06/~IP06_Character"),
    "cloud_kicker": (F, "~CK_Character"),
    "daisy": (F, "IP03/~IP03_Character"),
    "derpy_hooves": (F, "~DERP_Character"),
    "lemon_hearts": (U, "~IUC05_Character"),
    "lily_valley": (F, "~LY_Character"),
    "lotus_blossom": (F, "~Lot_B_Character"),
    "lyra_heartstrings": (U, "~IUC02_Character"),
    "minuette": (U, "~IUC04_Character"),
    "octavia": (M + "509_C04_Octavia", "~Octavia_Character"),
    "roseluck": (F, "RO/~RO_Character"),
    "sea_swirl": (U, "~IUC03_Character"),
    "starlight_glimmer": (M + "606_C03_Starlight_New", "~STL_Character"),
    "sunshower_raindrops": (F, "~SSR_Character"),
    "trixie": (M + "STK_C39_Trixie", "~TX_Character"),
    "twinkleshine": (U, "~IUC06_Character"),
    "vinyl_scratch": (F, "~DJP_Character"),
}
out = os.path.join(ROOT, "official_render")
os.makedirs(out, exist_ok=True)
tiles = []
for name, (xfl, sym) in MAP.items():
    d = os.path.join(out, "_svg", name)
    os.makedirs(d, exist_ok=True)
    for f in os.listdir(d):
        os.remove(os.path.join(d, f))
    r = subprocess.run([PY, "-m", "xfl2svg", os.path.join(ROOT, xfl), sym, d, "--last-frame", "1", "--center",
                        "--no-background", "--timeline-type", "symbol", "--width", "900", "--height", "900"],
                       capture_output=True, text=True)
    svgs = [f for f in os.listdir(d) if f.endswith(".svg")]
    if not svgs:
        print("нет", name, r.stderr[-300:], flush=True)
        continue
    svg = open(os.path.join(d, svgs[0]), encoding="utf-8").read()
    im = Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(svg_string=svg, width=900)))).convert("RGBA")
    bb = im.getbbox()
    if bb:
        im = im.crop(bb)
    im.save(os.path.join(out, name + ".png"))
    tiles.append((name, im))
    print("ok", name, flush=True)
T, C = 280, 7
font = ImageFont.truetype("arial.ttf", 17)
rows = (len(tiles) + C - 1) // C
sheet = Image.new("RGB", (C * T, rows * (T + 24)), (255, 255, 255))
dr = ImageDraw.Draw(sheet)
for i, (name, im) in enumerate(tiles):
    im = im.copy()
    im.thumbnail((T - 14, T - 14))
    x, y = (i % C) * T, (i // C) * (T + 24)
    sheet.paste(im, (x + (T - im.width) // 2, y + 24 + (T - im.height) // 2), im)
    dr.text((x + 6, y + 3), name, fill=(0, 0, 0), font=font)
sheet.save(os.path.join(out, "_official.png"))
json.dump({k: list(v) for k, v in MAP.items()}, open(os.path.join(out, "map.json"), "w"), indent=1)
print("лист", sheet.size)
