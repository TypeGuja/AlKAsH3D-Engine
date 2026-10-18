"""Шаблон полуоборота: у головы свои ключи (P.headKeys) и голова в персонаже всегда синхронизируется с кадром поворота."""
import sys
for p in sys.argv[1:]:
    s = open(p, encoding="utf-8").read()
    if "HEAD_NAME" in s:
        print("уже", p); continue
    def rep(a, b):
        global s
        assert a in s, (p, a[:60]); s = s.replace(a, b, 1)
    rep("function build(symName, byLayer, plan, S0) {", "var HEAD_NAME = null;\nfunction build(symName, byLayer, plan, S0) {")
    rep("if (e.elementType == \"instance\" && e.firstFrame == plan[k].src &&",
        "if (e.elementType == \"instance\" && (e.libraryItem.name == HEAD_NAME || e.firstFrame == plan[k].src) &&")
    rep("var P = JOB.ponies[pi], plan = makePlan(P.keys);",
        "var P = JOB.ponies[pi], plan = makePlan(P.keys), hplan = makePlan(P.headKeys || P.keys);\n    HEAD_NAME = findItem(doc, P.head).name;")
    rep("build(P.head, true, plan, S0);", "build(P.head, true, hplan, S0);")
    s = s.replace("brows(P.head, plan, S0);", "brows(P.head, hplan, S0);")
    open(p, "w", encoding="utf-8").write(s); print("ok", p)
