#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Пример‑игра «Вращающийся куб» на базе AlKAsH3D.
* Forward‑renderer (можно менять на "deferred", "hybrid" или "rtx").
* DX12‑бекенд (по умолчанию) – если в системе нет реальной
  библиотеки, движок переключится в «head‑less»‑режим с заглушками.
"""

from __future__ import annotations

import math
import numpy as np
import sys

# --------------------------------------------------------------
# 1️⃣  Импортируем публичные объекты из пакета
# --------------------------------------------------------------
from alkash3d import (
    Engine,
    Scene,
    Camera,
    DirectionalLight,
    PointLight,
    SpotLight,
    Mesh,
    Model,
    Node,
    Vec3,
    Vec4,
    Mat4,
    Quat,
)
from alkash3d.assets.material import PBRMaterial
from alkash3d.assets.texture_manager import TextureManager
from alkash3d.utils import logger

# --------------------------------------------------------------
# 2️⃣  Утилита – генератор куба (позиции, нормали, UV, индексы)
# --------------------------------------------------------------
def make_cube() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """
    Возвращает 4 numpy‑массива:
        vertices   – (N, 3)  float32
        normals    – (N, 3)  float32
        texcoords  – (N, 2)  float32
        indices    – (M,)    uint32   (по 3 индекса на треугольник)
    Для простоты создаём 24‑вершинный куб (по 4 вершины на каждый из 6
    квадрантов) – тогда нормали и UV правильно совпадают с каждой гранью.
    """
    # --- Позиции (координаты куба от -0.5 до +0.5) -----------------
    p = np.array(
        [
            #   x   y   z   (по 4 вершины на грань)
            # Front (+Z)
            [-0.5, -0.5, +0.5],
            [+0.5, -0.5, +0.5],
            [+0.5, +0.5, +0.5],
            [-0.5, +0.5, +0.5],
            # Back (‑Z)
            [+0.5, -0.5, -0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, +0.5, -0.5],
            [+0.5, +0.5, -0.5],
            # Left (‑X)
            [-0.5, -0.5, -0.5],
            [-0.5, -0.5, +0.5],
            [-0.5, +0.5, +0.5],
            [-0.5, +0.5, -0.5],
            # Right (+X)
            [+0.5, -0.5, +0.5],
            [+0.5, -0.5, -0.5],
            [+0.5, +0.5, -0.5],
            [+0.5, +0.5, +0.5],
            # Top (+Y)
            [-0.5, +0.5, +0.5],
            [+0.5, +0.5, +0.5],
            [+0.5, +0.5, -0.5],
            [-0.5, +0.5, -0.5],
            # Bottom (‑Y)
            [-0.5, -0.5, -0.5],
            [+0.5, -0.5, -0.5],
            [+0.5, -0.5, +0.5],
            [-0.5, -0.5, +0.5],
        ],
        dtype=np.float32,
    )

    # --- Нормали (по граней) ------------------------------------
    n = np.array(
        [
            # Front
            [0, 0, 1],
            [0, 0, 1],
            [0, 0, 1],
            [0, 0, 1],
            # Back
            [0, 0, -1],
            [0, 0, -1],
            [0, 0, -1],
            [0, 0, -1],
            # Left
            [-1, 0, 0],
            [-1, 0, 0],
            [-1, 0, 0],
            [-1, 0, 0],
            # Right
            [1, 0, 0],
            [1, 0, 0],
            [1, 0, 0],
            [1, 0, 0],
            # Top
            [0, 1, 0],
            [0, 1, 0],
            [0, 1, 0],
            [0, 1, 0],
            # Bottom
            [0, -1, 0],
            [0, -1, 0],
            [0, -1, 0],
            [0, -1, 0],
        ],
        dtype=np.float32,
    )

    # --- UV‑координаты -------------------------------------------
    uv = np.array(
        [
            # Каждая грань – квадрат (0,0) → (1,1)
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
        ] * 6,
        dtype=np.float32,
    )

    # --- Индексный массив (по 2 треугольника на грань) ---------
    # 0‑3 – первая грань, 4‑7 – вторая, …  (по 4 вершины на грань)
    indices = []
    for i in range(0, 24, 4):
        indices.extend([i, i + 1, i + 2, i, i + 2, i + 3])
    indices = np.array(indices, dtype=np.uint32)

    return p, n, uv, indices


# --------------------------------------------------------------
# 3️⃣  Кастомный Mesh‑класс, который вращается каждый кадр
# --------------------------------------------------------------
class RotatingCube(Mesh):
    """Куб, вращающийся вокруг оси Y.  Метод on_update вызывается в
    Engine.scene.update(dt)."""

    def __init__(self, *args, rotation_speed: float = 30.0, **kwargs):
        super().__init__(*args, **kwargs)
        self.rotation_speed = rotation_speed  # градусов в секунду

    def on_update(self, dt: float) -> None:
        # Обновляем локальный угол Y.
        self.rotation.y = (self.rotation.y + self.rotation_speed * dt) % 360.0


# --------------------------------------------------------------
# 4️⃣  Сборка сцены
# --------------------------------------------------------------
def build_scene() -> Scene:
    scene = Scene()

    # ---------- Камера (fly‑through) ----------
    cam = Camera(fov=70.0, near=0.1, far=200.0)
    cam.position = Vec3(0.0, 1.0, 3.0)     # стартовая позиция
    cam.rotation = Vec3(-15.0, 0.0, 0.0)   # немного наклоним вниз
    scene.add_child(cam)

    # ---------- Свет ----------
    # 1) Направленный (day‑light)
    sun = DirectionalLight(
        direction=Vec3(-0.7, -1.0, -0.4),
        color=Vec3(1.0, 0.95, 0.9),
        intensity=3.5,
    )
    scene.add_child(sun)

    # 2) Точечный свет – небольшая лампа
    lamp = PointLight(
        position=Vec3(2.0, 2.0, 0.0),   # позиция задаётся в world‑space (через трансформацию)
        radius=6.0,
        color=Vec3(1.0, 0.7, 0.4),
        intensity=2.0,
    )
    scene.add_child(lamp)

    # ---------- Куб ----------
    verts, norms, uvs, inds = make_cube()
    cube = RotatingCube(
        vertices=verts,
        normals=norms,
        texcoords=uvs,
        indices=inds,
        name="RotatingCube",
    )

    # Добавляем материал PBR (с альбедо‑цветом, без текстур)
    cube.material = PBRMaterial(
        albedo=(0.8, 0.2, 0.1, 1.0),   # красно‑оранжевый базовый цвет
        metallic=0.0,
        roughness=0.4,
        ao=1.0,
        emissive=(0.0, 0.0, 0.0),
    )
    scene.add_child(cube)

    # ---------- Пол (плоскость) ----------
    # Простейший паркетный пол можно сделать из двух треугольников.
    # В реальном проекте обычно грузятся из модели, но здесь сделаем вручную.
    floor_verts = np.array(
        [
            [-5.0, 0.0, -5.0],
            [+5.0, 0.0, -5.0],
            [+5.0, 0.0, +5.0],
            [-5.0, 0.0, +5.0],
        ],
        dtype=np.float32,
    )
    floor_norms = np.array([[0, 1, 0]] * 4, dtype=np.float32)
    floor_uvs = np.array([[0, 0], [1, 0], [1, 1], [0, 1]], dtype=np.float32)
    floor_inds = np.array([0, 1, 2, 0, 2, 3], dtype=np.uint32)

    floor = Mesh(
        vertices=floor_verts,
        normals=floor_norms,
        texcoords=floor_uvs,
        indices=floor_inds,
        name="Floor",
    )
    floor.material = PBRMaterial(
        albedo=(0.7, 0.7, 0.7, 1.0),   # светло‑серый пол
        metallic=0.0,
        roughness=0.9,
        ao=1.0,
    )
    scene.add_child(floor)

    # ---------- Привязываем камеру к сцене (чтобы Engine мог её обновлять) ----------
    scene.camera = cam
    return scene


# --------------------------------------------------------------
# 5️⃣  Основная точка входа – создаём Engine и запускаем игру
# --------------------------------------------------------------
def main() -> None:
    """
    Запуск демо‑игры.
    Параметры Engine:
        * renderer – one of: "forward", "deferred", "hybrid", "rtx"
        * backend_name – "dx12" (по‑умолчанию) или "gl"
    """
    # Выбираем рендерер; в этом демо удобно forward.
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – rotating cube demo",
        renderer="forward",          # попробуйте "deferred" / "hybrid" / "rtx"
        backend_name="dx12",        # если хотите OpenGL‑режим, поставьте "gl"
    )

    # Подменяем корневую сцену, построенную выше
    engine.scene = build_scene()

    # Запуск главного цикла
    engine.run()


# --------------------------------------------------------------
# 6️⃣  Точка входа скрипта
# --------------------------------------------------------------
if __name__ == "__main__":
    # Если скрипт запущен из IDE, можно поймать исключения
    # и вывести стек‑трейс в консоль, чтобы было удобно отлаживать.
    try:
        main()
    except Exception as exc:
        logger.error(f"[demo_game] Fatal error: {exc}")
        raise