"""Полная сборка карты Самары: python run_all.py

Нужно: Python 3.11+, numpy, scipy, shapely>=2.1, pyproj, pillow, tifffile, imagecodecs, osmium.
Исходные данные (если их нет в samara_map/_cache, скачиваются):
  * OSM: выгрузка Самарской области с download.openstreetmap.fr
  * рельеф: Copernicus DEM GLO-30, тайлы N53E049 и N53E050 (AWS Open Data)
"""
import os
import subprocess
import sys
import urllib.request

import config as C

SOURCES = {
    C.PBF: "https://download.openstreetmap.fr/extracts/russia/volga_federal_district/samara_oblast-latest.osm.pbf",
    **{C.CACHE / name: "https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_{t}_DEM/Copernicus_DSM_COG_10_{t}_DEM.tif"
       .format(t=name[4:-4]) for name in C.DEM_TILES},
}


def main():
    C.CACHE.mkdir(parents=True, exist_ok=True)
    for path, url in SOURCES.items():
        if not path.exists():
            print(f"[get] {url}")
            urllib.request.urlretrieve(url, path)
    here = C.Path(__file__).parent
    for step in ("extract_osm.py", "bld_levels.py", "terrain.py", "textures.py", "models.py", "build_chunks.py", "export_data.py"):
        print(f"=== {step}", flush=True)
        subprocess.run([sys.executable, step], check=True, cwd=here)
    # свет фонарей для движка — пишется Rust-инструментом структурами самого движка
    print("=== alfar_writer", flush=True)
    subprocess.run(["cargo", "run", "--release", "-q", "--", str(C.OUT / "lights"), str(C.OUT / "lights" / "samara_lamps.alfar")],
                   check=True, cwd=here / "alfar_writer",
                   env={**os.environ, "CARGO_TARGET_DIR": str(C.ROOT / "alkash3d-rust" / "target")})
    print("=== finalize.py", flush=True)
    subprocess.run([sys.executable, "finalize.py"], check=True, cwd=here)


if __name__ == "__main__":
    main()
