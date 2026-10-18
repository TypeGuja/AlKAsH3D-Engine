"""Задания Animate для всех официальных пони.

python make_turn_job.py keys      — JSFL рендера ключей поворота (с подписями DHX) для чтения углов
python make_turn_job.py turn      — JSFL полуоборота по файлам (нужен angles.json: имя -> [[кадр, угол], ...])
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
G = "G:/mlp_characters/mlp_2d/"
UPD = G + "official/misc/!Updated FLA's!/"
SOURCES = {
    "xfl/main_stk_c01_twilight_sparkle": UPD + "stk_c01_twilight_sparkle.fla",
    "xfl/main_stk_c04_applejack": UPD + "stk_c04_applejack.fla",
    "xfl/main_stk_c03_rainbow_dash": UPD + "stk_c03_rainbow_dash.fla",
    "xfl/main_stk_c02_pinkie_pie": UPD + "stk_c02_pinkie_pie.fla",
    "xfl/main_stk_c05_rarity": UPD + "stk_c05_rarity.fla",
    "xfl/main_stk_c06_fluttershy": UPD + "stk_c06_fluttershy.fla",
    "xfl/main_stk_c08_princess_celestia": UPD + "stk_c08_princess_celestia.fla",
    "xfl/main_STK_C28_Princess_Luna": UPD + "STK_C28_Princess_Luna.fla",
    "xfl/female": UPD + "stk_c29_Ponyville_Incidentals_Female.fla",
    "xfl/main_509_C04_Octavia": G + "official_puppets/dhx_main/509_C04_Octavia.fla",
    "xfl/main_606_C03_Starlight_New": G + "official_puppets/dhx_main/606_C03_Starlight_New.fla",
    "xfl/main_STK_C39_Trixie": UPD + "STK_C39_Trixie.fla",
}
OUT = "C:/Temp/claude/animate_render"
survey = json.load(open(os.path.join(HERE, "survey_all.json"), encoding="utf-8"))
todo = {n: s for n, s in survey.items() if s["xfl"] in SOURCES}
lib = os.path.join(HERE, "turn_template.jsfl")


def by_file():
    files = {}
    for n, s in todo.items():
        files.setdefault(SOURCES[s["xfl"]], []).append((n, s))
    return files


if sys.argv[1] == "keys":
    lines = []
    for fla, ps in by_file().items():
        lines.append(f'var doc = openDoc({json.dumps(fla)}); log("открыт " + doc.name);')
        for n, s in ps:
            a, ln = s["rot_at"], s["rot_len"]
            lines.append(f'renderSymbol(doc, {json.dumps(s["symbol"])}, {a + 1}, {a + ln}, "{OUT}/keys/{n}"); log("{n}");')
        lines.append("fl.closeDocument(doc, false);")
    open(os.path.join(HERE, "render_keys.jsfl"), "w", encoding="utf-8").write("\n".join(lines) + '\nlog("готово");\n')
    print("render_keys.jsfl:", len(todo), "пони")
elif sys.argv[1] == "turn":
    angles = json.load(open(os.path.join(HERE, "angles.json"), encoding="utf-8"))
    fixes = json.load(open(os.path.join(HERE, "fixes.json"), encoding="utf-8")) if os.path.exists(os.path.join(HERE, "fixes.json")) else {}
    tpl = open(lib, encoding="utf-8").read()
    for fla, ps in by_file().items():
        base = os.path.splitext(os.path.basename(fla))[0]
        ponies = []
        for n, s in ps:
            if n not in angles:
                print("нет углов:", n)
                continue
            head = s["heads"][0]
            folder = s["symbol"].rsplit("/", 1)[0] + "/" if "/" in s["symbol"] else ""
            ponies.append(dict(name=n, char=s["symbol"], head=head, keys=angles[n], **fixes.get(n, {})))
        if not ponies:
            continue
        job = dict(src=fla, dst=f"{OUT}/all/{base}_turn180.fla", render=f"{OUT}/all_render", ponies=ponies)
        open(os.path.join(HERE, f"job_{base}.jsfl"), "w", encoding="utf-8").write(tpl.replace("/*JOB*/null", json.dumps(job, ensure_ascii=False)))
        print(f"job_{base}.jsfl: {[p['name'] for p in ponies]}")
