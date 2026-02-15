# alkash3d/native/__init__.py
"""
Пакет‑заглушка для нативных (CUDA/OptiX) расширений.

Если у вас нет собранного нативного модуля `rt_core`, импортировать его всё‑равно можно
и работать в «чистом» OpenGL‑режиме.  При попытке вызвать любую функцию из `rt_core`
будет выброшено понятное RuntimeError‑сообщение, объясняющее, что нативный модуль
не найден/не собран.

Таким образом, остальные части движка (Engine, HybridRenderer, RTXRenderer и т.п.)
можно импортировать без дополнительных зависимостей.
"""

class _MissingRTCore:
    """
    Объект‑модуль‑заглушка.  Атрибуты запрашиваются динамически
    (`__getattr__`).  При обращении к любой функции/классу бросаем RuntimeError,
    указывая, что нативный модуль отсутствует.
    """
    def __getattr__(self, name):
        raise RuntimeError(
            f"Attempted to access native function/attribute '{name}' from "
            "`alkash3d.native.rt_core`, but the native extension is not "
            "available. If you need GPU‑based ray‑tracing, compile the native "
            "module according to the project’s README and place the resulting "
            "`rt_core` package on the Python path."
        )

# Публичный объект, импортируемый как:
#   from alkash3d.native import rt_core
rt_core = _MissingRTCore()
