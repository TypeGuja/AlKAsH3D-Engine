"""Этап 4: manifest.json + README.md в samara_map/."""
import json
import pickle
from collections import Counter

import config as C
from materials import MATERIALS

README = """# Карта Самары для AlKAsH3D

Сгенерировано `tools/samara_map` из открытых данных. Всё процедурное, никаких
снимков Google/Яндекса, поэтому можно использовать в игре (см. «Лицензии»).

## Что внутри

| Путь | Что это |
|---|---|
| `chunks/chunk_<gx>_<gz>.obj` | {n_chunks} чанков 256×256 м: рельеф с покрытием, дороги, тротуары, разметка, вода, здания с крышами, ж/д пути (балласт + рельсы), мосты с опорами, заборы/стены, платформы |
| фонарные столбы | **впечены в чанки** ({n_lamps} шт.: {lamps_by_kind}) — материалы `lamp_pole`, `lamp_housing`, `lamp_glass` |
| `lights/chunk_<gx>_<gz>.csv` | точки света фонарей (`x,y,z,kind`: `led` — магистрали, `sodium` — жилые улицы, `park` — аллеи) |
| `lights/samara_lamps_1km.alfar` | готовый свет для движка: фонари в радиусе ~1 км от центра карты (см. «Свет») |
| `props/chunk_<gx>_<gz>.csv` | инстансы деревьев по чанкам: `model,x,y,z,yaw_deg,scale` — {n_props} шт. ({props_by_model}) |
| `models/*.obj` | `street_lamp` (столб, тот же, что в чанках), `tree_deciduous`, `tree_pine` |
| `far_terrain.obj` | дальний рельеф (шаг {far} м, +25 км вокруг), под ближними чанками утоплен на {sink} м |
| `samara.mtl` | {n_mat} материалов (albedo + normal + roughness) |
| `textures/*.png` | процедурные бесшовные текстуры 1024² (roughness 512²) |
| `manifest.json` | метаданные: система координат, список чанков с границами и числом треугольников |

Всего треугольников в чанках: **{tris:,}**.

## Система координат

* Right-handed, **Y вверх**, метры — как в движке (`glam::camera::rh`) и редакторе.
* **X = восток, Z = юг** (то есть −север), **Y = высота над уровнем моря** (Волга ≈ 26–29 м).
* Начало координат (0, 0) — площадь Куйбышева ({lat}° с.ш., {lon}° в.д.),
  проекция поперечная Меркатора с центром там же (искажение < 0.01% на всей карте).
* Чанк `(gx, gz)` покрывает `X ∈ [gx·256, (gx+1)·256)`, `Z ∈ [gz·256, (gz+1)·256)` —
  та же сетка, что `EXPORT_CHUNK_SIZE_METERS = 256` в экспорте `.alworld` редактора.
* Вершины записаны в **мировых** координатах: чанк ставится с единичной трансформацией.
* Здание кладётся целиком в чанк, где лежит его представительная точка (как `split_mesh_by_chunk`),
  поэтому крупные здания могут выступать за границу своего чанка.
* Обход треугольников — CCW снаружи (стандарт OBJ), у каждой вершины есть `vn` и `vt`.
* В `props/*.csv` `yaw_deg` — поворот вокруг +Y против часовой (если смотреть сверху);
  у `street_lamp` рычаг смотрит по +X модели, после поворота он направлен к оси дороги.

## Свет

Столбы в геометрии чанков, а сам свет движок берёт из `.alfar`. Проставлять его руками не нужно:
`lights/samara_lamps_1km.alfar` уже сгенерирован по тем же позициям (прожекторы вниз из рассеивателей;
LED 4000K на магистралях, натриевые ~2000K во дворах, тёплые парковые; горят с 18:00 до 6:00,
около 2% натриевых «моргают»). Для `main_test` скопируй его в `alkash3d-rust/test.alfar`.

Почему только ~1 км, а не весь город: в движке сетка каллинга света (FirstFires) покрывает
±`far_plane` вокруг начала координат мира (в `main_test` это 1000 м), а обновление день/ночь
ищет каждый источник линейно. Другой центр/радиус:
`tools/samara_map/alfar_writer` → `samara_alfar lights/ out.alfar --center X,Z --radius 950 --max 2000`.

## Импорт

**В редактор AlKAsH3D — важно:** текущий `AssetLibrary::parse_obj`
(`alkash3d-editorapp/src/assets/library.rs`) склеивает все `o`-группы файла в ОДИН меш
и не читает `.mtl` (`let _ = materials`). То есть геометрия чанка импортируется,
а раскладка по материалам и текстурам потеряется. Чтобы текстуры доехали до движка,
импортёр нужно научить разбивать меш по `usemtl` и брать `map_Kd`/`map_Bump`,
либо писать сразу в `.alworld`/`.alwchunk`.

**В Blender** (для проверки глазами): File → Import → Wavefront OBJ, Forward = −Z, Up = Y.
Несколько соседних чанков импортируются и встают на свои места без сдвигов.

Нормали в `*_normal.png` — OpenGL-конвенция (+Y). Для DirectX-конвенции инвертируйте зелёный канал
(или `n.y = -n.y` в шейдере).

## Что это и чего тут нет

* Здания — объём по контуру OSM, этажность из `building:levels`/`height`. Где тегов нет
  (большинство), этажность оценивается по типу и площади, фасад выбирается эвристикой
  (панель / силикатный кирпич / охристый «исторический» / стекло / промышленный профлист /
  деревянный частный дом / гаражи). Крыши: плоские, двускатные и вальмовые у частного сектора,
  купола там, где в OSM есть `roof:shape=dome|onion`.
* Рельеф — Copernicus DEM (сетка 16 м по данным ~30 м). Это модель поверхности, поэтому
  под зданиями земля восстановлена интерполяцией, а в лесах вычтена оценка высоты крон (≈{canopy} м).
* Мосты — высота пролёта интерполируется между концами и поднимается не ниже просвета 5.5 м.
  Эстакады и развязки приближённые.
* Фонари: из OSM, где они отмечены, плюс сгенерированные вдоль дорог (магистрали — через 32 м,
  на широких с двух сторон; жилые улицы — через 40 м; освещённые аллеи — низкие столбы через 25 м).
* Нет: подземки, интерьеров, дорожных знаков и светофоров, ЛЭП, автомобилей, МАФов.
  Точность подписанного в OSM есть ровно та, что в OSM (данные от {osm_date}).

## Лицензии и атрибуция (обязательно указать в игре/титрах)

* Карта: © участники OpenStreetMap, лицензия ODbL — https://www.openstreetmap.org/copyright
* Рельеф: Copernicus DEM GLO-30 © DLR e.V. 2010–2014 и © Airbus Defence and Space GmbH 2014–2018,
  предоставлено в рамках COPERNICUS Европейским союзом и ЕКА.
* Текстуры и модели (включая фонарный столб) сгенерированы процедурно.

## Как пересобрать

```
cd tools/samara_map
python run_all.py            # все этапы; данные кэшируются в samara_map/_cache
```
Этапы по отдельности: `extract_osm.py` → `terrain.py` → `textures.py` → `models.py` →
`build_chunks.py` → `finalize.py`. Проверка без движка: `python preview.py gx0 gz0 gx1 gz1 out.png`.
"""


def main():
    region = pickle.load(open(C.WORK / "region.pkl", "rb"))
    stats = json.load(open(C.WORK / "chunk_stats.json"))
    lamps_by_kind = Counter()
    for f in (C.OUT / "lights").glob("*.csv"):
        for line in f.read_text().splitlines()[1:]:
            lamps_by_kind[line.rsplit(",", 1)[1]] += 1
    props_by_model = Counter()
    for f in (C.OUT / "props").glob("*.csv"):
        for line in f.read_text().splitlines()[1:]:
            props_by_model[line.split(",", 1)[0]] += 1
    tris = sum(sum(s["tris"].values()) for s in stats)
    chunks = []
    for s in sorted(stats, key=lambda s: (s["gx"], s["gz"])):
        gx, gz = s["gx"], s["gz"]
        chunks.append({
            "file": f"chunks/chunk_{gx}_{gz}.obj",
            "props": f"props/chunk_{gx}_{gz}.csv" if s["props"] else None,
            "grid": [gx, gz],
            "min": [gx * C.CHUNK, gz * C.CHUNK], "max": [(gx + 1) * C.CHUNK, (gz + 1) * C.CHUNK],
            "triangles": sum(s["tris"].values()),
            "props_count": s["props"],
            "lamps": s.get("lamps", 0),
        })
    import time
    osm_date = time.strftime("%Y-%m-%d", time.localtime((C.PBF).stat().st_mtime))
    manifest = {
        "name": "Samara (городской округ Самара + 1.5 км)",
        "axes": {"x": "east", "y": "up (meters above sea level)", "z": "south", "handedness": "right"},
        "origin": {"lat": C.ORIGIN_LAT, "lon": C.ORIGIN_LON, "label": "площадь Куйбышева"},
        "projection": C.PROJ,
        "chunk_size": C.CHUNK,
        "terrain_grid": C.GRID,
        "materials": sorted(MATERIALS),
        "models": ["models/street_lamp.obj", "models/tree_deciduous.obj", "models/tree_pine.obj"],
        "far_terrain": "far_terrain.obj",
        "total_triangles": tris,
        "props_by_model": dict(props_by_model),
        "lamps_by_kind": dict(lamps_by_kind),
        "lights_alfar": "lights/samara_lamps_1km.alfar",
        "attribution": ["© OpenStreetMap contributors (ODbL)",
                        "Copernicus DEM GLO-30 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018, provided under COPERNICUS by the EU and ESA"],
        "osm_data_date": osm_date,
        "chunks": chunks,
    }
    json.dump(manifest, open(C.OUT / "manifest.json", "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    from terrain import FOREST_CANOPY
    (C.OUT / "README.md").write_text(README.format(
        n_chunks=len(chunks), n_props=sum(props_by_model.values()),
        props_by_model=", ".join(f"{k}: {v}" for k, v in props_by_model.most_common()),
        n_lamps=sum(lamps_by_kind.values()),
        lamps_by_kind=", ".join(f"{k}: {v}" for k, v in lamps_by_kind.most_common()),
        far=int(C.FAR_GRID), sink=C.FAR_SINK, n_mat=len(MATERIALS), tris=tris,
        lat=C.ORIGIN_LAT, lon=C.ORIGIN_LON, canopy=int(FOREST_CANOPY), osm_date=osm_date), encoding="utf-8")
    print(f"[final] manifest: {len(chunks)} чанков, {tris:,} треугольников, props {dict(props_by_model)}")


if __name__ == "__main__":
    main()
