#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
Пример: вращающийся куб в AlKAsH3D.
"""

from __future__ import annotations

import numpy as np
from alkash3d import Engine
from alkash3d.scene import Mesh, DirectionalLight
from alkash3d.math.vec3 import Vec3
from alkash3d.utils import logger


# ----------------------------------------------------------------------
# Геометрия куба
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
    return verts, inds


# ----------------------------------------------------------------------
# Вращающийся куб
# ----------------------------------------------------------------------
class RotatingCube(Mesh):
    def __init__(self) -> None:
        verts, inds = make_cube()
        super().__init__(vertices=verts, indices=inds, name="Cube")
        logger.info("[RotatingCube] Created")

    def on_update(self, dt: float) -> None:
        """Вращаем куб вокруг оси Y со скоростью ~30°/сек."""
        self.rotation.y += 30.0 * dt
        if self.rotation.y > 360.0:
            self.rotation.y -= 360.0


# ----------------------------------------------------------------------
# Запуск Engine
# ----------------------------------------------------------------------
def main() -> None:
    logger.info("=" * 60)
    logger.info("Starting AlKAsH3D Engine")
    logger.info("=" * 60)

    # Конструируем Engine
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – вращающийся куб",
        renderer="forward",
        backend_name="dx12",
    )

    # Добавляем источник света
    sun = DirectionalLight(
        direction=Vec3(-0.5, -1.0, -0.5),
        color=Vec3(1.0, 1.0, 0.95),
        intensity=2.0,
    )
    engine.scene.add_child(sun)
    logger.info("[Main] Directional light added")

    # Добавляем куб
    cube = RotatingCube()
    engine.scene.add_child(cube)
    logger.info("[Main] Cube added to scene")

    # Настраиваем камеру
    engine.camera.position = Vec3(2.0, 1.5, 3.0)
    engine.camera.rotation = Vec3(-20.0, 45.0, 0.0)
    logger.info(f"[Main] Camera position: {engine.camera.position}")
    logger.info(f"[Main] Camera rotation: {engine.camera.rotation}")

    # Запускаем
    logger.info("[Main] Starting main loop...")
    engine.run()


if __name__ == "__main__":
    main()