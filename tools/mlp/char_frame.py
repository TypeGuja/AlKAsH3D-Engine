# Состав символа персонажа на заданном кадре: слои сверху вниз, что в них
# лежит (символ, матрица, точка трансформации, firstFrame).
# python char_frame.py <папка_xfl> <символ> <кадр> [<кадр>...]
import glob
import html
import os
import re
import sys
import xml.etree.ElementTree as ET

NS = "{http://ns.adobe.com/xfl/2008/}"


def sym_file(xfl, sym):
    p = os.path.join(xfl, "LIBRARY", *sym.split("/")) + ".xml"
    if os.path.exists(p):
        return p
    for f in glob.glob(os.path.join(xfl, "LIBRARY", "**", "*.xml"), recursive=True):
        head = open(f, encoding="utf-8", errors="replace").read(600)
        if f'name="{html.escape(sym, quote=True)}"' in head or f'name="{sym}"' in head:
            return f
    return None


def frame_at(layer, idx):
    for fr in layer.iter(NS + "DOMFrame"):
        i = int(fr.get("index"))
        d = int(fr.get("duration", "1"))
        if i <= idx < i + d:
            return fr
    return None


def describe(xfl, sym, idx):
    root = ET.parse(sym_file(xfl, sym)).getroot()
    out = []
    for layer in root.iter(NS + "DOMLayer"):
        if layer.get("layerType") in ("guide", "folder"):
            continue
        fr = frame_at(layer, idx)
        if fr is None:
            continue
        for el in fr.find(NS + "elements") or []:
            tag = el.tag.replace(NS, "")
            m = el.find(NS + "matrix/" + NS + "Matrix")
            mat = [float(m.get(k, d)) if m is not None else d for k, d in (("a", 1), ("b", 0), ("c", 0), ("d", 1), ("tx", 0), ("ty", 0))]
            tp = el.find(NS + "transformationPoint/" + NS + "Point")
            pt = (float(tp.get("x", 0)), float(tp.get("y", 0))) if tp is not None else None
            out.append((layer.get("name"), tag, el.get("libraryItemName"), el.get("firstFrame"), el.get("loop"), mat, pt))
    return out


if __name__ == "__main__":
    xfl, sym = sys.argv[1], sys.argv[2]
    for f in sys.argv[3:]:
        print(f"== кадр {f}")
        for layer, tag, name, ff, loop, mat, pt in describe(xfl, sym, int(f)):
            print(f"  {layer:10s} {tag:18s} {name or '':34s} ff={ff} {loop or ''} m={[round(x, 2) for x in mat]} pt={pt}")
