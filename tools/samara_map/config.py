"""Общие настройки генератора карты Самары.

Система координат выходных OBJ (совпадает с движком/редактором: right-handed,
Y вверх, метры):
    X = восток, Y = высота над уровнем моря (м), Z = юг (то есть -север).
Начало координат (0, 0) = площадь Куйбышева. Высота Y абсолютная (Волга ~28 м).
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]          # AlKAsH3D-Engine/
OUT = ROOT / "samara_map"
CACHE = OUT / "_cache"
WORK = CACHE / "work"

PBF = CACHE / "samara_oblast.osm.pbf"
DEM_TILES = {  # имя файла -> (lon0, lat0 верхнего левого пикселя)
    "dem_N53_00_E049_00.tif": (49.0, 54.0),
    "dem_N53_00_E050_00.tif": (50.0, 54.0),
}

ORIGIN_LAT = 53.1955
ORIGIN_LON = 50.1010
PROJ = f"+proj=tmerc +lat_0={ORIGIN_LAT} +lon_0={ORIGIN_LON} +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs"

BOUNDARY_REL = 287507          # "городской округ Самара"
REGION_BUFFER = 1500.0         # метров вокруг границы города тоже строим
BBOX_LONLAT = (49.60, 52.98, 50.52, 53.66)   # с запасом вокруг bbox округа

CHUNK = 256.0                  # = EXPORT_CHUNK_SIZE_METERS редактора
SUPER = 8                      # суперплитка = 8x8 чанков (2 км) — единица работы
GRID = 16.0                    # шаг сетки рельефа, м (DEM ~30 м)
BIN_MARGIN = 48.0              # запас при обрезке линий/площадей по суперплитке

FAR_GRID = 200.0               # шаг дальнего рельефа (горизонт), м
FAR_SINK = 4.0                 # насколько дальний рельеф утоплен под ближний

WORKERS = 3                    # процессов при сборке чанков (RAM ~5 ГБ свободно)


def chunk_of(x, z):
    import math
    return math.floor(x / CHUNK), math.floor(z / CHUNK)


def super_of_chunk(gx, gz):
    return gx // SUPER, gz // SUPER
