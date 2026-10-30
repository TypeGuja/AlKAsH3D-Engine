"""Автозамер подъезда: обходит точки маршрута, на каждой — время кадра на GPU
по проходам и снимок кадра, сравнивает с прошлым запуском.

Окно не показывается (ALKASH3D_HIDDEN) — прогон не мешает работать за
компьютером. Без vsync, полный экран 1920x1080 (как в игре), но скрыто.

    python tools/podezd_bench/bench.py            # замер + сравнение с прошлым
    python tools/podezd_bench/bench.py --save     # то же и сделать базой
    python tools/podezd_bench/bench.py --no-shots # без снимков (быстрее)

Результаты — tools/podezd_bench/out/: last.json, base.json, снимки *.png.
Сборка: cargo build --release --bin podezd (скрипт её не делает).
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EXE = ROOT / "alkash3d-rust" / "target" / "release" / "podezd.exe"
OUT = Path(__file__).resolve().parent / "out"

# Точки маршрута: имя, PODEZD_CAM (x,y,z,курс°,наклон°), доп. переменные.
# Окна на площадках — самые тяжёлые (двойные стёкла, двор, отражения).
POINTS = [
    ("вход", "1.25,1.62,0.55,180,-8", {}),
    ("марш_1эт", "1.8,2.3,2.6,180,-35", {}),
    ("марш_4эт", "1.8,10.7,2.6,180,-35", {}),
    ("площадка_дверь", "0.5,1.62,1.4,-35,-30", {}),
    ("окно_2эт", "0.81,5.82,5.00,204,-19", {}),
    ("окно_3эт", "0.77,8.62,4.92,202,-14", {}),
    ("поворот_головы", "1.8,4.4,3.0,0,10", {"PODEZD_SPIN": "0.8"}),
    ("тело_в_тенях", "1.25,1.62,0.55,180,-8", {"PODEZD_BODY": "1.25,0,0.55,180", "PODEZD_BODY_SPIN": "1.5"}),
    # двор перед домом (выход на улицу)
    ("улица_старт", "3.6,1.1,-5.6,-90,0", {}),
    ("улица_подъезд", "1.25,1.15,-4.2,180,5", {"PODEZD_DOOR": "95"}),
    ("улица_вдоль_дома", "-9,1.2,-5.5,-125,8", {}),
    ("улица_контейнеры", "22.5,1.15,-6.0,0,-8", {}),
    # проход через открытую дверь: видно и клетку, и двор
    ("проём_снаружи", "1.34,1.22,-2.06,177,0", {"PODEZD_DOOR": "95"}),
    ("проём_порог", "1.27,1.62,-0.47,183,-17", {"PODEZD_DOOR": "95"}),
    ("проём_изнутри", "1.94,1.62,0.75,205,-1", {"PODEZD_DOOR": "95"}),
    ("проём_лестница", "1.96,2.22,3.30,177,3", {"PODEZD_DOOR": "95"}),
    ("проём_назад", "1.25,1.62,1.2,0,-5", {"PODEZD_DOOR": "95"}),
]

# кадров на точку: при 300+ FPS меньше не выходит за прогрев первой секунды
FRAMES = 1500
# допуск: разница меньше — шум замера (нагрев, частоты видеокарты)
NOISE_MS = 0.4


def run_point(name, cam, extra, shots):
    env = dict(os.environ)
    env.update({
        "ALKASH3D_PERF": "1",
        "ALKASH3D_HIDDEN": "1",
        "PODEZD_VSYNC": "0",
        "PODEZD_FRAMES": str(FRAMES),
        "PODEZD_CAM": cam,
    })
    env.update(extra)
    if shots:
        env["ALKASH3D_SHOT"] = str(OUT / f"{name}.png")
        env["ALKASH3D_SHOT_FRAME"] = "150"
    p = subprocess.run([str(EXE)], cwd=EXE.parent.parent.parent, env=env, capture_output=True, timeout=180)
    text = p.stdout.decode("utf-8", errors="replace")
    gpu = [l for l in text.splitlines() if l.startswith("[PERF]   GPU")]
    if not gpu:
        print(f"  {name}: нет строки [PERF] GPU (код {p.returncode})", file=sys.stderr)
        return None
    # последняя секунда — прогрето, история TAA и адаптация глаза устоялись
    line = gpu[-1]
    total = float(re.search(r"GPU ([0-9.]+) мс", line).group(1))
    passes = {k.strip(): float(v) for k, v in re.findall(r"([^:,]+?) ([0-9.]+)(?:,|$)", line.split(":", 1)[1])}
    return {"total": total, "passes": passes}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--save", action="store_true", help="сделать этот замер базой для сравнения")
    ap.add_argument("--no-shots", action="store_true", help="без снимков кадра")
    args = ap.parse_args()
    if not EXE.exists():
        sys.exit(f"нет {EXE} — сначала cargo build --release --bin podezd")
    OUT.mkdir(exist_ok=True)
    base_path = OUT / "base.json"
    base = json.loads(base_path.read_text(encoding="utf-8")) if base_path.exists() else {}

    results = {}
    for name, cam, extra in POINTS:
        r = run_point(name, cam, extra, not args.no_shots)
        if r is None:
            continue
        results[name] = r
        b = base.get(name)
        diff = ""
        if b:
            d = r["total"] - b["total"]
            mark = "  хуже" if d > NOISE_MS else ("  лучше" if d < -NOISE_MS else "")
            diff = f"  (база {b['total']:.1f}, {d:+.1f}){mark}"
        top = sorted(r["passes"].items(), key=lambda kv: -kv[1])[:3]
        print(f"{name:16s} {r['total']:5.1f} мс{diff}   дороже всего: " + ", ".join(f"{k} {v:.1f}" for k, v in top))

    (OUT / "last.json").write_text(json.dumps(results, ensure_ascii=False, indent=1), encoding="utf-8")
    if args.save or not base:
        base_path.write_text(json.dumps(results, ensure_ascii=False, indent=1), encoding="utf-8")
        print(f"\nбаза сохранена: {base_path}")
    worst = max(results.items(), key=lambda kv: kv[1]["total"]) if results else None
    if worst:
        budget = 1000 / 60
        print(f"\nхудшая точка: {worst[0]} — {worst[1]['total']:.1f} мс, запас до 60 FPS {budget - worst[1]['total']:+.1f} мс")


if __name__ == "__main__":
    main()
