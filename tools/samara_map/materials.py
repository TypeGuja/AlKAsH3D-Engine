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
}
