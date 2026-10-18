# Поворот 0..180° по 3D-скелету (Луна, Селестия)

1. `blender -b --python pony3d.py -- marks <out.json>` и `-- joints <out2.json> <out.json>` (env `PONY=princess_luna`):
   проекции частей и суставов 3D-модели (G:/mlp_characters/mlp_3d) на 33 угла (yaw = 270 + угол DHX). Готовые: data/marks_*.json.
   Ещё режимы: `ids` (карта частей), `shade` (обычный рендер углов), `bones`, `list`.
2. Новые промежуточные ключи (67.5/112.5/157.5, у Селестии ещё 22.5 со сложенными крыльями): jobs/prep9_*.jsfl
   (= view_lib.jsfl + prep; makeView: копия ключа + сдвиг частей + подмена внутренних кадров частей).
3. Пошаговый поворот на 9 ключей: turn_template + jobs/job(B)_*.jsfl через batch_turn.sh (noTween).
4. 3D-сборка каждого кадра: `var MARKS = <data/marks_*.json>;` + jobs/*_full_A|B.jsfl + drive3d_head.jsfl + drive3d_v3.jsfl → run_jsfl.py.
   Ноги — поворот+растяжение между суставом (плечо/бедро) и копытом; голова — свой кадр головы на каждый кадр
   поворота (headStart+2i), черты лица сдвигаются по 3D с ослаблением (HEAD_DAMP).
5. Экспорт: export_official.py twilight_sparkle <пони> (MLPO_XFL=<пони>=<xfl>) → донор; ../mlpo_replace.py вливает в общий пакет.
Проверка: turnsheet.py (лист 33 кадров + GIF; BG=#888 для белых пони).
Аура Луны: в исходнике нужен патч вырожденной матрицы PL_HornMagicAura (см. память luna-rotation-sources).

v2 (2026-10-15): хвост/грива не растягиваются по 3D (только сдвиг за корнем); blendToward подтягивает группы частей
к соседнему ключу (вес до 0.5 на смене рисунка; грива — только если тот же рисунок: символ+firstFrame);
слой ANGLE не копируется; fix_angles.jsfl переписывает подписи углов. Эмоции: export_official FEATURES без учёта регистра
(BLINK_*, LIPSYNC/Lypsync, Brow), CHEEK — только с символом Cheek/Blush. Проверка паков на CPU: pack_turn.py, face_preview.py.
