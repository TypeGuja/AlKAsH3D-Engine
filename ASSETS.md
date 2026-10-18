# Assets / Ассеты

## In the repository / В репозитории — `assets/mlp/` (Git LFS)

Third-party files, see LICENSE §3. / Чужие файлы, см. LICENSE §3.

### Pony pack / Пак пони — `assets/mlp/pack/`

| File / Файл | Size / Размер | SHA-256 |
|---|---|---|
| `all_v15c.mlpo` (32 ponies / пони) | 2 319 001 430 | `1f7195783148bd67a676edaafa458f872cbc6fb5b7e0eac27fcd9c9234565c31` |

**EN:** GitHub LFS rejects files over 2 GB, so the pack is split into `part00` and
`part01`. Run `join.bat` (Windows) or `join.sh` once to get `all_v15c.mlpo`, then
start the game with `PONIES_OFFICIAL=<path>/assets/mlp/pack/all_v15c.mlpo`.
The joined file is gitignored.

**RU:** GitHub LFS не принимает файлы больше 2 ГБ, поэтому пак разрезан на `part00`
и `part01`. Один раз запусти `join.bat` (Windows) или `join.sh`, чтобы получить
`all_v15c.mlpo`. Потом запускай игру с
`PONIES_OFFICIAL=<path>/assets/mlp/pack/all_v15c.mlpo`.
Склеенный файл в git не попадает.

### Animate files / Файлы Animate — `assets/mlp/fla/`

**EN:** The pack is built from these latest versions: 0..180° turns, plus the
3D-driven turns of Luna and Celestia. Rebuild it with
`tools/mlp/export_official.py` + `tools/mlp/mlpo_replace.py --compact`.

**RU:** Из этих последних версий собран пак: повороты 0..180° и 3D-повороты Луны
и Селестии. Пересобрать пак: `tools/mlp/export_official.py` +
`tools/mlp/mlpo_replace.py --compact`.

| File / Файл | Size / Размер |
|---|---|
| STK_C23_Princess_Twilight_Sparkle_turn180ab.fla | 4 031 925 |
| stk_c01_twilight_sparkle_turn180ab.fla | 1 139 005 |
| stk_c04_applejack_turn180.fla | 1 038 731 |
| stk_c02_pinkie_pie_turn180.fla | 1 363 389 |
| stk_c05_rarity_turn180ab.fla | 5 349 163 |
| stk_c06_fluttershy_turn180ab.fla | 2 196 163 |
| stk_c03_rainbow_dash_turn180ab.fla | 2 427 121 |
| STK_C28_Princess_Luna_turn180ab.fla | 2 317 449 |
| stk_c08_princess_celestia_turn180ab.fla | 8 572 601 |
| 509_C04_Octavia_turn180.fla | 4 037 009 |
| 606_C03_Starlight_New_turn180ab.fla | 5 448 930 |
| STK_C39_Trixie_turn180ab.fla | 2 642 215 |
| stk_c29_Ponyville_Incidentals_Female_turn180_d.fla | 22 771 399 |
| stk_c29_Ponyville_Incidentals_Female_turn180_e.fla | 22 432 467 |
| unicorns_turn180_v4.fla | 4 250 414 |
| STK_C28_Princess_Luna_turn3d_v6.fla | 2 367 169 |
| stk_c08_princess_celestia_turn3d_v2.fla | 8 607 121 |

**EN:** All intermediate versions are kept locally in `G:/mlp_characters/mlp_2d/animate_work/`.
The DHX sources are in `G:/mlp_characters/mlp_2d/official_puppets/`, `official/` and `xfl/`.

**RU:** Все промежуточные версии лежат локально в `G:/mlp_characters/mlp_2d/animate_work/`.
Исходники DHX: `G:/mlp_characters/mlp_2d/official_puppets/`, `official/`, `xfl/`.

## Not in the repository / Не в репозитории

### Script data / Данные скриптов

**EN:** `tools/mlp/turn3d/data/` (`marks_*.json`, `model_*.json`) is derived from
3D pony models (`G:/mlp_characters/mlp_3d`). Regenerate it with `pony3d.py`
(see `tools/mlp/turn3d/README.md`).

**RU:** `tools/mlp/turn3d/data/` (`marks_*.json`, `model_*.json`) получены из 3D-моделей
пони (`G:/mlp_characters/mlp_3d`). Пересоздаются через `pony3d.py`
(см. `tools/mlp/turn3d/README.md`).

### Buckshot Roulette scenes / Сцены Buckshot Roulette

**EN:** `data/buckshot/` (scene, scene_hq, restroom, void) was extracted from the
game and is not included. The default paths are set in code; override them with
`BUCKSHOT_DATA`, `RESTROOM_SCENE`, `RESTROOM_VOID`.

**RU:** `data/buckshot/` (scene, scene_hq, restroom, void) извлечены из игры и в
репозиторий не входят. Пути по умолчанию задаются в коде, переопределяются через
`BUCKSHOT_DATA`, `RESTROOM_SCENE`, `RESTROOM_VOID`.
