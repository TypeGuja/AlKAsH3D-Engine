# Все ракурсы персонажа из пакета (CPU, как движок): python pack_turn.py pack out.png имя [с какого] [сколько]
import sys
import numpy as np
from PIL import Image, ImageDraw
import face_preview as fp
pack, out, who = sys.argv[1:4]
v0 = int(sys.argv[4]) if len(sys.argv) > 4 else 1
nv = int(sys.argv[5]) if len(sys.argv) > 5 else 33
b, tex, chars = fp.read(pack, who) if fp.read.__code__.co_argcount > 1 else fp.read(pack)
name, hgt, views = next(c for c in chars if c[0] == who)
e = dict(eyes="normal", blink=0, mood="happy", ph="rest", brow=None, blush=None)
T = 260; cols = 11; rows = (nv + cols - 1) // cols
sh = Image.new("RGB", (T * cols, (T + 14) * rows), "white"); d = ImageDraw.Draw(sh)
for k in range(nv):
    fr, ang, parts = views[v0 + k]
    im = fp.render(b, tex, parts, e, T=T, k=T / 3.2)
    x, y = (k % cols) * T, (k // cols) * (T + 14)
    sh.paste(im.convert("RGB"), (x, y + 14)); d.text((x + 3, y + 1), f"{fr} {np.degrees(ang):.1f}", fill="red")
sh.save(out); print(out, sh.size, len(views))
