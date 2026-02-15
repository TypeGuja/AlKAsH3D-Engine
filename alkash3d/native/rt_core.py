# alkash3d/native/rt_core.py
"""
Псевдо‑реализация API `rt_core` без настоящего CUDA/OptiX‑ядра.

Функция `trace` просто возвращает пустой (чёрный) RGBA‑массив нужного
размера.  Это достаточно, чтобы остальная часть движка могла вызвать
`rt_core.trace(...)` без падения, хотя реального рендеринга не будет.
"""

import numpy as np

def trace(*,
          width: int,
          height: int,
          cam_pos,
          cam_dir,
          cam_up,
          cam_right,
          bvh=None,
          output_texture=None):
    """
    Псевдо‑трассировка.

    Параметры полностью совпадают с теми, которые ожидает
    оригинальный `alkash3d.renderer.raytracer.RayTracer`.  Мы просто
    возвращаем массив байтов, заполненный нулями (чёрный фон).

    Возврат:
        bytes – последовательность длиной width*height*4 (RGBA8).
    """
    # Заполняем чёрным (0) изображение
    rgba = np.zeros((height, width, 4), dtype=np.uint8)
    # Если нужен «демо‑график», можно добавить простой градиент:
    # y‑координата → интенсивность в R‑канале, например.
    # for y in range(height):
    #     rgba[y, :, 0] = np.linspace(0, 255, width, dtype=np.uint8)
    return rgba.tobytes()
