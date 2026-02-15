# -*- coding: utf-8 -*-
"""
alkash3d_rtx/_kernel.py

Небольшой CUDA‑kernel, используемый в пакете alkash3d_rtx.
Он не претендует на «полноценный» трассировщик – лишь демонстрирует,
что модуль действительно умеет выполнять вычисления на GPU и
возвращать готовый RGBA‑буфер.

API:
    render_image(width, height,
                cam_pos, cam_target, cam_up) -> bytes

* cam_pos, cam_target, cam_up – кортежи (x, y, z) в системе float.
* Возвращаемый объект – bytes‑строка длиной width*height*4 (RGBA8),
  готовая к загрузке в DirectX 12‑текстуру.

Если CUDA‑устройства нет, функция просто возвращает полностью
чёрный кадр того же размера (fallback‑режим), чтобы движок не
падал.
"""

from __future__ import annotations

import json
import math
from typing import Tuple

import numpy as np

# ----------------------------------------------------------------------
# Попытка импортировать Numba‑CUDA. Если не удаётся – переходим в
# «CPU‑fallback».  При отсутствии CUDA мы всё‑равно должны предоставить
# функцию render_image, но она будет генерировать чёрный буфер.
# ----------------------------------------------------------------------
try:
    from numba import cuda, float32, uint8
    _CUDA_AVAILABLE = True
except Exception:                      # Numba не установлен, драйвер не найден и т.п.
    cuda = None                         # type: ignore
    _CUDA_AVAILABLE = False

# ----------------------------------------------------------------------
# CUDA‑kernel (будет скомпилирован JIT‑компилятором Numba)
# ----------------------------------------------------------------------
if _CUDA_AVAILABLE:

    @cuda.jit
    def rt_kernel(
        width: int, height: int,
        cam_pos: float32[:], cam_dir: float32[:],
        cam_up: float32[:], cam_right: float32[:],
        out_img: uint8[:, :, :]
    ) -> None:
        """
        Простой ray‑marcher.

        Параметры
        ----------
        width, height : Размеры кадра.
        cam_pos        : (3,) массив float32 – позиция камеры.
        cam_dir        : (3,) массив float32 – «взгляд» камеры (нормализованный).
        cam_up         : (3,) массив float32 – up‑вектор камеры (нормализованный).
        cam_right      : (3,) массив float32 – right‑вектор камеры (нормализованный).
        out_img        : (height, width, 4) массив uint8 – куда пишем RGBA.
        """
        x, y = cuda.grid(2)                # координаты пикселя
        if x >= width or y >= height:
            return

        # --------------------------------------------------------------
        # Переводим координаты пикселя в Normalized Device Coordinates
        # --------------------------------------------------------------
        ndc_x = (2.0 * x) / width  - 1.0       # диапазон [-1, 1]
        ndc_y = 1.0 - (2.0 * y) / height      # инвертируем Y (верх = +Y)

        # --------------------------------------------------------------
        # Строим направление луча в мировом пространстве:
        #    ray_dir = cam_dir + ndc_x * cam_right + ndc_y * cam_up
        # --------------------------------------------------------------
        ray_dir_x = cam_dir[0] + ndc_x * cam_right[0] + ndc_y * cam_up[0]
        ray_dir_y = cam_dir[1] + ndc_x * cam_right[1] + ndc_y * cam_up[1]
        ray_dir_z = cam_dir[2] + ndc_x * cam_right[2] + ndc_y * cam_up[2]

        # --------------------------------------------------------------
        # Нормализуем direction
        # --------------------------------------------------------------
        norm = math.sqrt(
            ray_dir_x * ray_dir_x +
            ray_dir_y * ray_dir_y +
            ray_dir_z * ray_dir_z
        )
        ray_dir_x /= norm
        ray_dir_y /= norm
        ray_dir_z /= norm

        # --------------------------------------------------------------
        # Тестируем пересечение с единичной сферой в начале координат.
        # Уравнение сферы: |p|^2 = r^2, где r = 1.
        # Луч: o + t * d,   o – позиция камеры,   d – нормализованный dir.
        # --------------------------------------------------------------
        # вектор от камеры к центру сферы (центр = 0,0,0)
        oc_x = cam_pos[0]
        oc_y = cam_pos[1]
        oc_z = cam_pos[2]

        a = ray_dir_x * ray_dir_x + ray_dir_y * ray_dir_y + ray_dir_z * ray_dir_z
        b = 2.0 * (oc_x * ray_dir_x + oc_y * ray_dir_y + oc_z * ray_dir_z)
        c = oc_x * oc_x + oc_y * oc_y + oc_z * oc_z - 1.0   # r^2 = 1

        disc = b * b - 4.0 * a * c

        if disc > 0.0:
            # Попали в сферу → ярко‑оранжевый пиксель
            out_img[y, x, 0] = 255   # R
            out_img[y, x, 1] = 120   # G
            out_img[y, x, 2] = 30    # B
            out_img[y, x, 3] = 255   # A
        else:
            # Фоновый вертикальный градиент (от тёмно‑синего к чуть светлее)
            # ndc_y уже в диапазоне [-1, 1]; преобразуем к [0, 1]
            t = 0.5 * (ndc_y + 1.0)
            base = int(30 + 25 * t)               # небольшая вариация синего канала
            out_img[y, x, 0] = base                # R (чуть меняется)
            out_img[y, x, 1] = base                # G (чуть меняется)
            out_img[y, x, 2] = 30 + int(50 * t)    # B (градиент)
            out_img[y, x, 3] = 255                # A (полностью непрозрачный)

else:
    # ----------------------------------------------------------------------
    # Если CUDA недоступен – объявляем пустую функцию‑заглушку,
    # чтобы импортировать модуль без ошибок.
    # ----------------------------------------------------------------------
    def rt_kernel(*_):
        """Пустая заглушка – будет использована только в CPU‑fallback."""
        pass


# ----------------------------------------------------------------------
# Вспомогательная функция – формирует векторы камеры (pos, dir, up, right)
# ----------------------------------------------------------------------
def _make_camera_vectors(
    cam_pos: Tuple[float, float, float],
    cam_target: Tuple[float, float, float],
    cam_up: Tuple[float, float, float],
) -> Tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """
    Возвращает четыре вектора (float32) в виде numpy‑массивов:
        * cam_pos   – позиция камеры
        * cam_dir   – нормализованное направление взгляда
        * cam_up    – нормализованный up‑вектор
        * cam_right – правый вектор (векторное произведение dir × up)

    Результат полностью совместим с тем, что ожидает CUDA‑kernel.
    """
    pos = np.array(cam_pos, dtype=np.float32)

    # Направление от cam_pos к cam_target
    target = np.array(cam_target, dtype=np.float32)
    dir_vec = target - pos
    dir_norm = np.linalg.norm(dir_vec)
    if dir_norm == 0.0:
        dir_norm = 1.0
    dir_vec /= dir_norm

    # Приводим up к ортогональному к dir
    up_vec = np.array(cam_up, dtype=np.float32)
    up_vec -= dir_vec * np.dot(up_vec, dir_vec)
    up_norm = np.linalg.norm(up_vec)
    if up_norm == 0.0:
        up_norm = 1.0
    up_vec /= up_norm

    # Правый вектор – cross(dir, up)
    right_vec = np.cross(dir_vec, up_vec).astype(np.float32)

    return pos, dir_vec, up_vec, right_vec


# ----------------------------------------------------------------------
# Публичный интерфейс – вызывается из alkash3d_rtx.__init__
# ----------------------------------------------------------------------
def render_image(
    width: int,
    height: int,
    cam_pos: Tuple[float, float, float],
    cam_target: Tuple[float, float, float],
    cam_up: Tuple[float, float, float],
) -> bytes:
    """
    Генерирует изображение (RGBA8) указанного размера.

    Parameters
    ----------
    width, height : int
        Размер кадра.
    cam_pos, cam_target, cam_up : tuple of 3 floats
        Параметры камеры.  В случае, если вы передаёте JSON‑строку,
        их обычно берут из поля ``camera`` (см. alkash3d_rtx.__init__).

    Returns
    -------
    bytes
        RGBA‑буфер (width*height*4 байт), готовый к передаче в
        DirectX 12‑текстуру.
    """
    # -----------------------------------------------------------------
    # 1️⃣ Формируем векторы, которые нужны ядру
    # -----------------------------------------------------------------
    pos, dir_vec, up_vec, right_vec = _make_camera_vectors(
        cam_pos, cam_target, cam_up
    )

    # -----------------------------------------------------------------
    # 2️⃣ Выделяем массив под результат
    # -----------------------------------------------------------------
    img = np.zeros((height, width, 4), dtype=np.uint8)

    # -----------------------------------------------------------------
    # 3️⃣ Если CUDA доступна – запускаем kernel, иначе просто возвращаем
    #    чистый чёрный кадр (fallback)
    # -----------------------------------------------------------------
    if _CUDA_AVAILABLE:
        # Размеры блока/грида – достаточно 16×16 (можно менять)
        threads_per_block = (16, 16)
        blocks_x = (width + threads_per_block[0] - 1) // threads_per_block[0]
        blocks_y = (height + threads_per_block[1] - 1) // threads_per_block[1]

        # Переносим массив в device‑memory
        d_img = cuda.device_array_like(img)

        # Запуск kernel
        rt_kernel[
            (blocks_x, blocks_y), threads_per_block
        ](
            width, height,
            pos, dir_vec, up_vec, right_vec,
            d_img
        )

        # Копируем результат обратно в host‑массив
        d_img.copy_to_host(img)
    else:
        # Фолбэк‑режим: заполняем полностью чёрным (можно добавить градиент,
        # если хотите, но основной смысл — гарантировать работоспособность).
        img[:, :, :] = 0

    # -----------------------------------------------------------------
    # 4️⃣ Возвращаем готовый буфер в виде bytes
    # -----------------------------------------------------------------
    return img.tobytes()


# ----------------------------------------------------------------------
# Удобный импорт из пакета:
# В __init__.py достаточно написать:
#
#   from ._kernel import render_image
#
# ----------------------------------------------------------------------
__all__ = ["render_image"]
