# bobber.py — эталонный пример .py-скрипта для встроенного Python
# hot-reload интерпретатора движка (см. engine/scripting_python.rs).
#
# Соглашение по протоколу — ТО ЖЕ САМОЕ, что и у Lua (bobber.lua в
# alkash3d-luascript/examples/), намеренно, чтобы один и тот же сценарий
# был взаимозаменяем между всеми языками скриптинга:
#   update(dt, x, y, z, rx, ry, rz) -> (new_x, new_y, new_z, changed)
#     вызывается каждый кадр; changed=1, если позицию нужно применить.
#   on_event(event_type, data0, data1, data2, data3)
#     вызывается по событию (см. ScriptEventType в scripting_api.rs).
#
# Особенность именно Python-варианта: этот файл можно редактировать ПРЯМО
# во время работы движка — при следующем кадре движок заметит изменение
# mtime файла и автоматически перезагрузит его (см.
# PythonScriptRuntime::check_hot_reload) — без перезапуска игры.

import math

_base = None
_time_accum = 0.0
_amplitude = 0.3   # метры
_bob_speed = 2.0   # рад/сек


def update(dt, x, y, z, rx, ry, rz):
    global _base, _time_accum

    # Точка покоя фиксируется один раз, при первом кадре.
    if _base is None:
        _base = (x, y, z)

    _time_accum += dt
    offset_y = math.sin(_time_accum * _bob_speed) * _amplitude

    return (_base[0], _base[1] + offset_y, _base[2], 1)


def on_event(event_type, data0, data1, data2, data3):
    global _amplitude

    # ZoneEnter (3) или Custom (0): data0 > 0 — новый множитель амплитуды.
    if event_type == 3 or event_type == 0:
        if data0 > 0.0:
            _amplitude = 0.3 * data0
