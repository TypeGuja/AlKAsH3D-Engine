# File: examples 2/main.py
#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
Пример‑демонстрация AlKAsH3D:

* Вращающийся куб без использования текстур.
* Обычное направляющее освещение (DirectionalLight).
* Управление камерой – WASD + мышь (fly‑through).
* F9 — отображать FPS, F10 — переключить V‑Sync.
"""

from __future__ import annotations

import math
import sys

import numpy as np

# ----------------------------------------------------------------------
# Публичный API движка
# ----------------------------------------------------------------------
from alkash3d import (
    Engine,            # главный цикл + окно
    DirectionalLight,  # простой свет
    Mesh,              # геометрический объект
    Vec3,              # вектор‑позиции
)

# ----------------------------------------------------------------------
# Функция‑фабрика: создаём меш‑куб.
# Вершины передаются отдельными массивами (позиции + нормали);
# UV‑координаты не нужны, т.к. в примере нет текстур.
# ----------------------------------------------------------------------
def create_cube_mesh() -> Mesh:
    """
    Возвращает объект `Mesh`, представляющий единичный куб
    (центр (0,0,0), длина ребра = 1).  Позиции и нормали задаются
    отдельными массивами – именно так ожидает `Mesh`.
    """
    # ------------------- Позиции -------------------
    positions = np.array(
        [
            -0.5, -0.5, -0.5,   # 0
             0.5, -0.5, -0.5,   # 1
             0.5,  0.5, -0.5,   # 2
            -0.5,  0.5, -0.5,   # 3
            -0.5, -0.5,  0.5,   # 4
             0.5, -0.5,  0.5,   # 5
             0.5,  0.5,  0.5,   # 6
            -0.5,  0.5,  0.5,   # 7
        ],
        dtype=np.float32,
    )

    # ------------------- Нормали -------------------
    # Для простоты используем нормали, направленные от центра к вершине.
    raw_normals = np.array(
        [
            -1, -1, -1,
             1, -1, -1,
             1,  1, -1,
            -1,  1, -1,
            -1, -1,  1,
             1, -1,  1,
             1,  1,  1,
            -1,  1,  1,
        ],
        dtype=np.float32,
    )
    # Нормализуем каждую нормаль
    normals = raw_normals.reshape((-1, 3))
    normals = normals / np.linalg.norm(normals, axis=1, keepdims=True)
    normals = normals.ravel()

    # ------------------- Индексы -------------------
    indices = np.array(
        [
            0, 1, 2, 0, 2, 3,   # back
            4, 6, 5, 4, 7, 6,   # front
            0, 4, 5, 0, 5, 1,   # bottom
            3, 2, 6, 3, 6, 7,   # top
            1, 5, 6, 1, 6, 2,   # right
            0, 3, 7, 0, 7, 4,   # left
        ],
        dtype=np.uint32,
    )

    # `Mesh` лениво создаст GPU‑буферы при первом draw()
    return Mesh(vertices=positions, normals=normals, indices=indices)


# ----------------------------------------------------------------------
# Точка входа
# ----------------------------------------------------------------------
def main() -> int:
    """
    Создаём движок, сцену и запускаем простой цикл.
    Текстур в примере нет – включена встроенная белая placeholder‑текстура.
    """
    # ---------- 1️⃣ Engine (создаёт окно, DX12‑бэкенд и ForwardRenderer)
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – rotating cube (no textures)",
        renderer="forward",    # ForwardRenderer
        backend_name="dx12",   # DX12‑бэкенд (можно сменить на "gl")
    )
    win   = engine.window    # окно уже привязано к бекенду
    scene = engine.scene
    cam   = engine.camera

    # ---------- 2️⃣ Куб
    cube = create_cube_mesh()
    cube.position = Vec3(0.0, 0.0, 0.0)   # центрируем в начале мира
    scene.add_child(cube)

    # ---------- 3️⃣ Направляющий свет
    sun = DirectionalLight(direction=Vec3(-0.5, -1.0, -0.3))
    scene.add_child(sun)

    # ---------- 4️⃣ Параметры анимации
    angular_speed = math.radians(30.0)   # 30°/сек (по оси Y)

    # ---------- 5️⃣ Главный цикл
    try:
        while not win.should_close():
            dt = engine.timer.tick()           # время кадра
            win.poll_events()                # обработка ввода
            cam.update_fly(dt, win.input)    # WASD + мышь

            # вращаем куб
            cube.rotation.y += angular_speed * dt

            # рендерим кадр
            engine.renderer.render(scene, cam)

            # Present (DX12) / swap buffers (GL)
            win.swap_buffers()
    finally:
        # ---------- 6️⃣ Очистка
        engine.shutdown()
        win.close()

    return 0


# ----------------------------------------------------------------------
# Старт скрипта
# ----------------------------------------------------------------------
if __name__ == "__main__":
    sys.exit(main())
