"""Таблица материалов: размер тайла текстуры в метрах (u, v), шероховатость, металличность.

Сборщик чанков берёт отсюда масштаб UV, генератор текстур — список, что рисовать,
а samara.mtl пишется по этой же таблице.
"""

MATERIALS = {
    # --- земля
    "grass":            dict(tile=(4, 4),     rough=0.95, metal=0.0),
    "grass_urban":      dict(tile=(6, 6),     rough=0.95, metal=0.0),
    "meadow":           dict(tile=(6, 6),     rough=0.95, metal=0.0),
    "forest_floor":     dict(tile=(4, 4),     rough=0.95, metal=0.0),
    "farmland":         dict(tile=(8, 8),     rough=0.95, metal=0.0),
    "dirt":             dict(tile=(4, 4),     rough=0.9,  metal=0.0),
    "sand":             dict(tile=(4, 4),     rough=0.9,  metal=0.0),
    "gravel":           dict(tile=(3, 3),     rough=0.9,  metal=0.0),
    "asphalt":          dict(tile=(6, 6),     rough=0.8,  metal=0.0),
    "paving":           dict(tile=(2.4, 2.4), rough=0.75, metal=0.0),
    "concrete":         dict(tile=(4, 4),     rough=0.85, metal=0.0),
    "pitch":            dict(tile=(4, 4),     rough=0.9,  metal=0.0),
    "tartan":           dict(tile=(4, 4),     rough=0.85, metal=0.0),
    "water":            dict(tile=(8, 8),     rough=0.05, metal=0.0),
    "far_ground":       dict(tile=(256, 256), rough=0.95, metal=0.0),
    # --- дороги / ж/д
    "road_marking":     dict(tile=(1, 1),     rough=0.6,  metal=0.0),
    "ballast":          dict(tile=(4.4, 3.3), rough=0.9,  metal=0.0),   # u поперёк пути (0..1), v вдоль
    "rail_steel":       dict(tile=(1, 1),     rough=0.35, metal=1.0),
    # --- фасады (тайл = 2 пролёта x 2 этажа, кроме указанных)
    "facade_panel":     dict(tile=(6.4, 5.6), rough=0.8,  metal=0.0, floor=2.8),
    "facade_brick":     dict(tile=(6.0, 6.0), rough=0.85, metal=0.0, floor=3.0),
    "facade_historic":  dict(tile=(8.0, 8.0), rough=0.85, metal=0.0, floor=4.0),
    "facade_commercial": dict(tile=(8.0, 8.0), rough=0.6, metal=0.0, floor=4.0),
    "facade_glass":     dict(tile=(6.0, 7.2), rough=0.15, metal=0.3, floor=3.6),
    "wall_industrial":  dict(tile=(8.0, 6.0), rough=0.6,  metal=0.6, floor=6.0),
    "wall_wood":        dict(tile=(6.0, 3.0), rough=0.85, metal=0.0, floor=3.0),
    "wall_garage":      dict(tile=(6.0, 3.0), rough=0.8,  metal=0.0, floor=2.8),
    "wall_brick_plain": dict(tile=(3.0, 3.0), rough=0.85, metal=0.0),
    "concrete_fence":   dict(tile=(4.0, 2.5), rough=0.85, metal=0.0),
    "fence_metal":      dict(tile=(2.0, 2.0), rough=0.5,  metal=0.8),
    "hedge":            dict(tile=(2.0, 2.0), rough=0.9,  metal=0.0),
    # --- крыши
    "roof_flat":        dict(tile=(8, 8),     rough=0.9,  metal=0.0),
    "roof_metal":       dict(tile=(4, 4),     rough=0.45, metal=0.8),
    "roof_tile":        dict(tile=(3, 3),     rough=0.7,  metal=0.0),
    "dome_gold":        dict(tile=(2, 2),     rough=0.25, metal=1.0),
    # --- фонарные столбы
    "lamp_pole":        dict(tile=(0.6, 3.0), rough=0.45, metal=1.0),   # оцинкованная сталь
    "lamp_housing":     dict(tile=(1, 1),     rough=0.5,  metal=0.2),   # корпус светильника, RAL 7035
    "lamp_glass":       dict(tile=(1, 1),     rough=0.15, metal=0.0),   # матовый рассеиватель
    # --- растительность / пропсы
    "bark":             dict(tile=(1, 2),     rough=0.9,  metal=0.0),
    "leaves":           dict(tile=(2, 2),     rough=0.8,  metal=0.0),
    "pine_needles":     dict(tile=(2, 2),     rough=0.85, metal=0.0),

    # --- детали улиц из OSM (osm_details.py). size — сторона текстуры в пикселях:
    # мелким предметам 1024² не нужно, а каждый материал — это ~12 МБ VRAM при 1024².
    # tile=None — UV 0..1 на всю грань (картинка знака/двери целиком).
    "steel_grey":       dict(tile=(1, 1),     rough=0.5,  metal=0.7, size=256),   # окрашенные столбы, кронштейны
    "steel_lattice":    dict(tile=(1, 1),     rough=0.55, metal=0.9, size=256),   # оцинкованные опоры ЛЭП
    "cable":            dict(tile=(1, 1),     rough=0.45, metal=0.6, size=128),   # провода (алюминий/медь, потемневшие)
    "insulator":        dict(tile=(1, 1),     rough=0.2,  metal=0.0, size=128),   # стекло/фарфор изоляторов
    "signal_body":      dict(tile=(1, 1),     rough=0.5,  metal=0.1, size=128),   # корпус светофора (тёмный пластик)
    "signal_lens":      dict(tile=(1, 1),     rough=0.1,  metal=0.0, size=128),   # линзы (выключены: циклы светофоров не моделируются)
    "sign_crossing":    dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 5.19.1 «Пешеходный переход»
    "sign_give_way":    dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 2.4 «Уступите дорогу»
    "sign_stop":        dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 2.5 «Движение без остановки запрещено»
    "sign_bus_stop":    dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 5.16 «Место остановки автобуса/троллейбуса»
    "sign_tram_stop":   dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 5.17 «Место остановки трамвая»
    "sign_no_stopping": dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 3.27 «Остановка запрещена» (по Mapillary)
    "sign_priority":    dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 2.1 «Главная дорога» (по Mapillary)
    "sign_parking":     dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # 6.4 «Парковка» (по Mapillary)
    "sign_atlas":       dict(tile=None,       rough=0.4,  metal=0.0, size=2048),  # все знаки ГОСТ по снимкам (sign_atlas.py)
    "sign_metro":       dict(tile=None,       rough=0.4,  metal=0.0, size=256),   # красная «М» над входом в метро
    "sign_back":        dict(tile=(1, 1),     rough=0.6,  metal=0.5, size=128),   # оборот знака (оцинковка)
    "sign_board":       dict(tile=(1, 1),     rough=0.4,  metal=0.1, size=128),   # фон вывесок (чёрный, как фон атласа букв)
    # буквы вывесок: атлас глифов, светятся (подсветка). ke — яркость в единицах
    # движка: подсвеченная вывеска ~500 кд/м², а рассеиватель фонаря (~25 000 кд/м²)
    # в движке = intensity/площадь ≈ 240 => 500 кд/м² ≈ 5. Шейдер умножает ke на
    # albedo, поэтому светятся только буквы, а чёрный фон глифа — нет.
    "glyphs_white":     dict(tile=None,       rough=0.3,  metal=0.0, size=1024, ke=(5.0, 5.0, 5.0)),
    "glyphs_yellow":    dict(tile=None,       rough=0.3,  metal=0.0, size=1024, ke=(5.0, 5.0, 5.0)),
    "glyphs_red":       dict(tile=None,       rough=0.3,  metal=0.0, size=1024, ke=(5.0, 5.0, 5.0)),
    "glyphs_green":     dict(tile=None,       rough=0.3,  metal=0.0, size=1024, ke=(5.0, 5.0, 5.0)),
    "glyphs_blue":      dict(tile=None,       rough=0.3,  metal=0.0, size=1024, ke=(5.0, 5.0, 5.0)),
    # адресные таблички (синяя эмаль, белые буквы): без ke — не подсвечены
    "glyphs_addr":      dict(tile=None,       rough=0.35, metal=0.0, size=1024),
    "addr_plate":       dict(tile=(1, 1),     rough=0.35, metal=0.0, size=64),
    "addr_frame":       dict(tile=(1, 1),     rough=0.35, metal=0.0, size=64),
    "door_metal":       dict(tile=None,       rough=0.45, metal=0.6, size=256),   # подъездная металлическая дверь
    "entrance_lamp_glass": dict(tile=(1, 1),  rough=0.2,  metal=0.0, size=128),   # плафон над подъездом (светится от своего источника)
    "glass_shelter":    dict(tile=(1, 1),     rough=0.05, metal=0.0, size=128),   # стекло павильона остановки
    "granite":          dict(tile=(1, 1),     rough=0.55, metal=0.0, size=256),   # бордюры, постаменты, чаши фонтанов
    "bronze":           dict(tile=(1, 1),     rough=0.4,  metal=1.0, size=256),   # скульптуры, доски
    "wood_planks":      dict(tile=(1, 1),     rough=0.8,  metal=0.0, size=256),   # скамейки, столы
    "paint_red":        dict(tile=(1, 1),     rough=0.5,  metal=0.2, size=128),   # гидранты, шлагбаумы
    "paint_blue":       dict(tile=(1, 1),     rough=0.5,  metal=0.2, size=128),   # почтовые ящики (Почта России)
    "paint_yellow":     dict(tile=(1, 1),     rough=0.5,  metal=0.2, size=128),   # игровое оборудование
    "paint_green":      dict(tile=(1, 1),     rough=0.6,  metal=0.2, size=128),   # мусорные контейнеры
    "boom_stripes":     dict(tile=(1.0, 1),   rough=0.5,  metal=0.1, size=128),   # стрела шлагбаума (красно-белая, по 0.5 м)
}
