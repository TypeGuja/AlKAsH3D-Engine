# Заменить персонажей в пакете .mlpo (v2) их версией из другого пакета — без полного переэкспорта.
#   python mlpo_replace.py <пакет> <донор> <выход> имя [имя ...] [--compact]
#   --compact: выкинуть текстуры, на которые никто не ссылается (старые версии заменённых).
# Текстуры пакета копируются потоком; текстуры донора, нужные заменяемым персонажам, дописываются в конец
# (индексы перенумеровываются). Старые текстуры заменённых остаются (не мешают).
import struct
import sys

CHUNK = 1 << 24


def rd(f, fmt):
    return struct.unpack(fmt, f.read(struct.calcsize(fmt)))


def read_pack(path):
    f = open(path, "rb")
    assert f.read(4) == b"MLPO"
    ver, tint, nt = rd(f, "<IfI")
    assert ver == 2, f"версия {ver}"
    tex = []
    for _ in range(nt):
        w, h = rd(f, "<II")
        tex.append((f.tell(), w, h))
        f.seek(w * h * 4, 1)
    (nc,) = rd(f, "<I")
    chars = []
    for _ in range(nc):
        (n,) = rd(f, "<I"); name = f.read(n).decode("utf-8")
        height, nv = rd(f, "<fI")
        views = []
        for _ in range(nv):
            fr, ang, npart = rd(f, "<ifI")
            parts = []
            for _ in range(npart):
                ti, role = rd(f, "<II"); (sl,) = rd(f, "<I"); sym = f.read(sl)
                aff = rd(f, "<6f"); piv = rd(f, "<2f")
                kind, ns = rd(f, "<II")
                st = [(rd(f, "<I")[0], rd(f, "<6f")) for _ in range(ns)]
                parts.append([ti, role, sym, aff, piv, kind, st])
            views.append((fr, ang, parts))
        chars.append([name, height, views])
    return f, tint, tex, chars


def copy(src, off, n, dst):
    src.seek(off)
    while n:
        b = src.read(min(n, CHUNK)); dst.write(b); n -= len(b)


def main():
    args = sys.argv[1:]
    compact = "--compact" in args
    args = [x for x in args if x != "--compact"]
    base, donor, out, *names = args
    fb, tint, tb, cb = read_pack(base)
    fd, _, td, cd = read_pack(donor)
    new = {c[0]: c for c in cd if c[0] in names}
    miss = [n for n in names if n not in new]
    assert not miss, f"нет в доноре: {miss}"
    # текстуры донора, на которые ссылаются заменяемые
    used = sorted({i for c in new.values() for _, _, ps in c[2] for p in ps for i in [p[0]] + [s[0] for s in p[6]] if i != 0xFFFFFFFF})
    remap = {i: len(tb) + k for k, i in enumerate(used)}
    for c in new.values():
        for _, _, ps in c[2]:
            for p in ps:
                p[0] = remap.get(p[0], p[0])
                p[6] = [(remap[i] if i != 0xFFFFFFFF else i, a) for i, a in p[6]]
    chars = [new.get(c[0], c) for c in cb]
    # источники текстур в порядке нового пакета: (файл, смещение, w, h)
    src = [(fb, off, w, h) for off, w, h in tb] + [(fd,) + td[i] for i in used]
    if compact:
        live = sorted({i for c in chars for _, _, ps in c[2] for p in ps for i in [p[0]] + [s[0] for s in p[6]] if i != 0xFFFFFFFF})
        rm = {i: k for k, i in enumerate(live)}
        for c in chars:
            for _, _, ps in c[2]:
                for p in ps:
                    p[0] = rm.get(p[0], p[0])
                    p[6] = [(rm[i] if i != 0xFFFFFFFF else i, a) for i, a in p[6]]
        print(f"компактно: текстур {len(src)} → {len(live)}")
        src = [src[i] for i in live]
    with open(out, "wb") as fo:
        fo.write(b"MLPO" + struct.pack("<IfI", 2, tint, len(src)))
        for f, off, w, h in src:
            fo.write(struct.pack("<II", w, h)); copy(f, off, w * h * 4, fo)
        fo.write(struct.pack("<I", len(chars)))
        for name, height, views in chars:
            nb = name.encode("utf-8")
            fo.write(struct.pack("<I", len(nb)) + nb + struct.pack("<fI", height, len(views)))
            for fr, ang, ps in views:
                fo.write(struct.pack("<ifI", fr, ang, len(ps)))
                for ti, role, sym, aff, piv, kind, st in ps:
                    fo.write(struct.pack("<II", ti, role) + struct.pack("<I", len(sym)) + sym + struct.pack("<6f", *aff) + struct.pack("<2f", *piv))
                    fo.write(struct.pack("<II", kind, len(st)))
                    for i, a in st:
                        fo.write(struct.pack("<I", i) + struct.pack("<6f", *a))
    for n, c in new.items():
        print(f"{n}: ракурсов {len(c[2])}")
    print(f"текстур +{len(used)}, персонажей {len(chars)} →", out)


if __name__ == "__main__":
    main()
