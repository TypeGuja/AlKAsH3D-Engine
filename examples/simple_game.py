#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
Пример: вращающийся куб в AlKAsH3D.

* Forward‑рендерер (самый простой)
* DX12‑бэкенд (если DLL доступен – реальное GPU‑рисование, иначе stub‑режим)
* Один DirectionalLight и куб без текстур (используется placeholder‑текстура).
"""

from __future__ import annotations

import numpy as np
from alkash3d import Engine, select_backend
from alkash3d.scene import Mesh, DirectionalLight, Camera
from alkash3d.math.vec3 import Vec3
from alkash3d.utils import logger


# ----------------------------------------------------------------------
# Простой материал для теста (не используем PBRMaterial)
# ----------------------------------------------------------------------
class SimpleMaterial:
    """Простой материал для теста - использует белую текстуру-заглушку"""

    def __init__(self, color=(1.0, 1.0, 1.0, 1.0)):
        self.albedo = color
        self.color = color
        logger.info(f"[SimpleMaterial] Created with color {color}")

    def bind(self, backend):
        # Ничего не делаем - будет использована белая текстура-заглушка
        # Это нормально, просто передаем управление дальше
        pass


# ----------------------------------------------------------------------
# 1️⃣ Геометрия куба (позиции + индексы)
# ----------------------------------------------------------------------
def make_cube() -> tuple[np.ndarray, np.ndarray]:
    """Возвращает (vertices, indices) обычного единичного куба."""
    verts = np.array(
        [
            [-0.5, -0.5, -0.5],
            [+0.5, -0.5, -0.5],
            [+0.5, +0.5, -0.5],
            [-0.5, +0.5, -0.5],
            [-0.5, -0.5, +0.5],
            [+0.5, -0.5, +0.5],
            [+0.5, +0.5, +0.5],
            [-0.5, +0.5, +0.5],
        ],
        dtype=np.float32,
    )

    inds = np.array(
        [
            0, 1, 2, 0, 2, 3,  # -Z
            4, 6, 5, 4, 7, 6,  # +Z
            0, 4, 5, 0, 5, 1,  # -Y
            3, 2, 6, 3, 6, 7,  # +Y
            1, 5, 6, 1, 6, 2,  # +X
            0, 3, 7, 0, 7, 4,  # -X
        ],
        dtype=np.uint32,
    )

    logger.info(f"[make_cube] Created cube with {len(verts)} vertices, {len(inds)} indices")
    logger.info(f"[make_cube] Vertex range X: {verts[:, 0].min():.2f} to {verts[:, 0].max():.2f}")
    logger.info(f"[make_cube] Vertex range Y: {verts[:, 1].min():.2f} to {verts[:, 1].max():.2f}")
    logger.info(f"[make_cube] Vertex range Z: {verts[:, 2].min():.2f} to {verts[:, 2].max():.2f}")

    return verts, inds


# ----------------------------------------------------------------------
# 2️⃣ Класс Mesh‑куба, вращающийся каждый кадр
# ----------------------------------------------------------------------
class RotatingCube(Mesh):
    def __init__(self) -> None:
        verts, inds = make_cube()
        super().__init__(vertices=verts, indices=inds, name="Cube")
        # Используем простой материал вместо PBRMaterial
        self.material = SimpleMaterial(color=(0.8, 0.2, 0.2, 1.0))  # Красный куб
        logger.info("[RotatingCube] Created with SimpleMaterial (red)")

    def on_update(self, dt: float) -> None:
        """Вращаем куб вокруг оси Y со скоростью ~30°/сек."""
        self.rotation.y += 30.0 * dt
        if self.rotation.y > 360.0:
            self.rotation.y -= 360.0
        # Для отладки - выводим поворот раз в секунду
        if int(self.rotation.y) % 30 == 0:
            logger.debug(f"[RotatingCube] Rotation: {self.rotation.y:.1f}°")


# ----------------------------------------------------------------------
# 3️⃣ Запуск Engine
# ----------------------------------------------------------------------
def main() -> None:
    logger.info("=" * 60)
    logger.info("Starting AlKAsH3D Engine")
    logger.info("=" * 60)

    # ------------------------------------------------------------------
    #   Конструируем Engine (окно 1280×720, forward‑renderer, DX12‑бэкенд)
    # ------------------------------------------------------------------
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – вращающийся куб",
        renderer="forward",  # forward / deferred / hybrid / rtx
        backend_name="dx12",  # если DLL отсутствует — будет работать stub‑режим
    )

    # ------------------------------------------------------------------
    #   Добавляем источник света
    # ------------------------------------------------------------------
    sun = DirectionalLight(
        direction=Vec3(-0.5, -1.0, -0.5),  # направление *к* сцене
        color=Vec3(1.0, 1.0, 0.95),
        intensity=3.0,
    )
    engine.scene.add_child(sun)
    logger.info("[Main] Directional light added")

    # ------------------------------------------------------------------
    #   Добавляем куб
    # ------------------------------------------------------------------
    cube = RotatingCube()
    engine.scene.add_child(cube)
    logger.info("[Main] Cube added to scene")

    # ------------------------------------------------------------------
    #   Настраиваем камеру, чтобы видеть куб
    # ------------------------------------------------------------------
    # Отодвигаем камеру назад и немного вверх/вбок
    engine.camera.position = Vec3(2.0, 1.5, 3.0)
    engine.camera.rotation = Vec3(-20.0, 45.0, 0.0)  # Поворачиваем, чтобы смотреть на куб
    logger.info(f"[Main] Camera position: {engine.camera.position}")
    logger.info(f"[Main] Camera rotation: {engine.camera.rotation}")
    logger.info(f"[Main] Camera forward: {engine.camera.forward}")

    # ------------------------------------------------------------------
    #   Запускаем главный цикл
    # ------------------------------------------------------------------
    logger.info("[Main] Starting main loop...")
    engine.run()


if __name__ == "__main__":
    main()