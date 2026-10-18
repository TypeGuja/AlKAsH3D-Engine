# Для каждой пони из roster.json: метки кадров символа персонажа (ракурсы/
# позы), число кадров, вложенные части и наличие костей (IK/armature).
import glob
import html
import json
import os
import re

ROOT = "G:/mlp_2d"
roster = json.load(open(os.path.join(ROOT, "roster.json"), encoding="utf-8"))
FAN = {"blossomforth": "xfl/fan_Blossom_Forth", "cloudchaser": "xfl/fan_Cloudchaser",
       "flitter": "xfl/fan_Flitter", "sunset_shimmer": "xfl/fan_Sunset_Shimmer"}


def sym_file(xfl, sym):
    """Файл символа в LIBRARY: имя с '/' -> вложенные папки."""
    p = os.path.join(ROOT, xfl, "LIBRARY", *sym.split("/")) + ".xml"
    if os.path.exists(p):
        return p
    # Имена с недопустимыми символами Animate кодирует — ищем по атрибуту name.
    for f in glob.glob(os.path.join(ROOT, xfl, "LIBRARY", "**", "*.xml"), recursive=True):
        head = open(f, encoding="utf-8", errors="replace").read(600)
        if f'name="{html.escape(sym, quote=True)}"' in head or f'name="{sym}"' in head:
            return f
    return None


def has_bones(xfl):
    hits = 0
    for f in glob.glob(os.path.join(ROOT, xfl, "**", "*.xml"), recursive=True):
        t = open(f, encoding="utf-8", errors="replace").read()
        hits += len(re.findall(r"<IKTree|DOMIKPose|<IKBone|isArmature|armature", t, re.I))
    return hits


cache = {}
for name, r in roster.items():
    if r["source"] == "official":
        xfl, sym = r["xfl"], r["symbol"]
    else:
        xfl, sym = FAN[name], None
    if xfl not in cache:
        cache[xfl] = has_bones(xfl)
    if sym is None:
        # Фанатские: все символы "... ► ALL" (ракурсы) и их метки.
        alls = []
        for f in glob.glob(os.path.join(ROOT, xfl, "LIBRARY", "**", "*ALL*.xml"), recursive=True):
            t = open(f, encoding="utf-8", errors="replace").read()
            labels = sorted(set(html.unescape(x) for x in re.findall(r'name="([^"]+)"[^>]*labelType', t)))
            alls.append(os.path.basename(f)[:-4] + f"[{len(labels)} меток]")
        print(f"{name:22s} фан  кости:{cache[xfl]}  ракурсы-символы: {', '.join(sorted(alls))}")
        continue
    f = sym_file(xfl, sym)
    t = open(f, encoding="utf-8", errors="replace").read() if f else ""
    labels = [html.unescape(x) for x in re.findall(r'<DOMFrame[^>]*name="([^"]+)"', t)]
    frames = max([int(x) for x in re.findall(r'<DOMFrame index="(\d+)"', t)] or [0]) + 1
    print(f"{name:22s} офиц кости:{cache[xfl]}  кадров:{frames}  метки: {' | '.join(dict.fromkeys(labels))[:400]}")
