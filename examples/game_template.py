#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
Простейшая 3D‑сцена с вращающимся кубом на AlKAsH3D Engine (DX12, forward).
Управление камерой: WASD + мышь (fly‑камера движка).
"""

import sys
from pathlib import Path

import numpy as np

# Добавляем корень репозитория в sys.path
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from alkash3d import Engine, Mesh, Vec3, DirectionalLight  # type: ignore


def create_cube_mesh(size: float = 1.0) -> Mesh:
    """
    Создаём куб через Mesh.
    Вершины: только позиции (x, y, z) – stride 12 байт, как сейчас ожидает DX12‑обёртка.
    """
    s = size * 0.5

    # 8 вершин (позиции)
    vertices = np.array(
        [
            [-s, -s, -s],  # 0
            [s, -s, -s],   # 1
            [s, s, -s],    # 2
            [-s, s, -s],   # 3
            [-s, -s, s],   # 4
            [s, -s, s],    # 5
            [s, s, s],     # 6
            [-s, s, s],    # 7
        ],
        dtype=np.float32,
    )

    # 12 треугольников (36 индексов)
    indices = np.array(
        [
            # Задняя грань (‑Z)
            0, 1, 2,  0, 2, 3,
            # Передняя грань (+Z)
            4, 6, 5,  4, 7, 6,
            # Левая грань (‑X)
            0, 3, 7,  0, 7, 4,
            # Правая грань (+X)
            1, 5, 6,  1, 6, 2,
            # Нижняя грань (‑Y)
            0, 4, 5,  0, 5, 1,
            # Верхняя грань (+Y)
            3, 2, 6,  3, 6, 7,
        ],
        dtype=np.uint32,
    )

    cube = Mesh(vertices=vertices, indices=indices, name="Cube")
    cube.position = Vec3(0.0, 0.0, 0.0)
    return cube


class CubeGame:
    def __init__(self):
        # Запускаем движок: DX12 + forward‑renderer
        self.engine = Engine(
            width=1280,
            height=720,
            title="AlKAsH3D – 3D Cube",
            renderer="forward",
            backend_name="dx12",
        )

        # Куб в центре сцены
        self.cube = create_cube_mesh(size=1.5)
        self.engine.scene.add_child(self.cube)

        # Простейший «солнечный» свет
        sun = DirectionalLight(direction=Vec3(-0.3, -1.0, -0.2))
        sun.intensity = 2.0
        self.engine.scene.add_child(sun)

        # Стартовая позиция камеры
        self.engine.camera.position = Vec3(0.0, 1.5, 5.0)

        # Вращение куба каждый кадр
        def cube_update(dt: float, node=self.cube):
            node.rotation.y += 45.0 * dt  # 45° в секунду

        # Scene.update вызывает on_update(dt) у всех нод, если он есть
        self.cube.on_update = cube_update  # type: ignore[attr-defined]

    def run(self):
        self.engine.run()


if __name__ == "__main__":
    game = CubeGame()
    game.run()