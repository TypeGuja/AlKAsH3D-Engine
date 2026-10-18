"""Распаковать итоговые .fla и выгрузить общий пакет всех 31 официальных пони."""
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
G = "G:/mlp_characters/mlp_2d"
Z = "C:/Program Files/7-Zip/7z.exe"
PY = sys.executable
files = json.load(open(os.path.join(HERE, "final_files.json")))
survey = json.load(open(os.path.join(HERE, "survey_all.json"), encoding="utf-8"))
SRC = {  # исходный xfl (из roster) -> итоговый файл
    "xfl/main_stk_c01_twilight_sparkle": "stk_c01_twilight_sparkle", "xfl/main_stk_c04_applejack": "stk_c04_applejack",
    "xfl/main_stk_c02_pinkie_pie": "stk_c02_pinkie_pie", "xfl/main_stk_c05_rarity": "stk_c05_rarity",
    "xfl/main_stk_c06_fluttershy": "stk_c06_fluttershy", "xfl/main_stk_c03_rainbow_dash": "stk_c03_rainbow_dash",
    "xfl/main_STK_C28_Princess_Luna": "STK_C28_Princess_Luna", "xfl/main_stk_c08_princess_celestia": "stk_c08_princess_celestia",
    "xfl/main_509_C04_Octavia": "509_C04_Octavia", "xfl/main_606_C03_Starlight_New": "606_C03_Starlight_New",
    "xfl/main_STK_C39_Trixie": "STK_C39_Trixie", "xfl/female": "stk_c29_Ponyville_Incidentals_Female", "xfl/unicorns": "unicorns_turn180", "xfl/main_STK_C23_Princess_Twilight_Sparkle": "STK_C23_Princess_Twilight_Sparkle",
}
dirs = {}
for f in files:
    base = os.path.splitext(os.path.basename(f))[0]
    d = f"{G}/xfl/final_{base}"
    subprocess.run(["cmd", "/c", "rmdir", "/s", "/q", d.replace("/", "\\")], capture_output=True)
    subprocess.run([Z, "x", "-y", f"-o{d}", f], capture_output=True)
    for k, v in SRC.items():
        if base.startswith(v) and not base.endswith("_e"):
            dirs[k] = d
    if base.endswith("Female_turn180_e"):
        dir_e = d
    print("распакован", base, flush=True)
over = []
for n, s in survey.items():
    if s["xfl"] in dirs:
        # Aloe, Lotus, Vinyl пересобраны отдельно (у DHX лысые ракурсы) — свой файл _e.
        over.append(f"{n}={dir_e if n in ('aloe', 'lotus_blossom', 'vinyl_scratch') else dirs[s['xfl']]}")
names = ["twilight_sparkle"] + sorted(n for n in survey if n != "twilight_sparkle")
env = dict(os.environ, MLPO_XFL=";".join(over), PYTHONIOENCODING="utf-8")
print("пони:", len(names), flush=True)
r = subprocess.run([PY, f"{G}/tools/export_official.py", *names, "--out", f"{G}/engine/all_v3.mlpo", "--sheet"], env=env, cwd=f"{G}/tools")
sys.exit(r.returncode)
