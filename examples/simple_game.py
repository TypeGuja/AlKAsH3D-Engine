# examples/simple_game.py
# -*- coding: utf-8 -*-

import sys
import os
import numpy as np
from alkash3d import Engine, logger
from alkash3d.scene import Mesh, DirectionalLight
from alkash3d.math.vec3 import Vec3


def make_cube():
    """Создаёт простой куб."""
    # Вершины куба
    vertices = np.array([
        # Front face
        [-1, -1, 1],  # 0
        [1, -1, 1],  # 1
        [1, 1, 1],  # 2
        [-1, 1, 1],  # 3
        # Back face
        [-1, -1, -1],  # 4
        [1, -1, -1],  # 5
        [1, 1, -1],  # 6
        [-1, 1, -1],  # 7
    ], dtype=np.float32)

    # Индексы
    indices = np.array([
        # Front face
        0, 1, 2, 2, 3, 0,
        # Back face
        4, 5, 6, 6, 7, 4,
        # Top face
        3, 2, 6, 6, 7, 3,
        # Bottom face
        0, 1, 5, 5, 4, 0,
        # Left face
        0, 3, 7, 7, 4, 0,
        # Right face
        1, 2, 6, 6, 5, 1,
    ], dtype=np.uint32)

    logger.info(f"[make_cube] Created cube with {len(vertices)} vertices, {len(indices)} indices")
    return Mesh(vertices, indices, name="Cube")


class RotatingCube:
    """Вращающийся куб."""

    def __init__(self, mesh, speed=50.0):
        self.mesh = mesh
        self.speed = speed
        self.rotation = Vec3(0, 0, 0)
        logger.info("[RotatingCube] Created")

    def on_update(self, dt):
        """Обновляет вращение куба."""
        self.rotation.y += self.speed * dt
        self.rotation.x += self.speed * dt * 0.5
        self.mesh.rotation = self.rotation

    def draw(self, backend):
        """Отрисовывает куб."""
        self.mesh.draw(backend)


def main():
    """Главная функция."""
    print("=" * 60)
    print("Starting AlKAsH3D Engine")
    print("=" * 60)

    # Создаём движок
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D Engine - Simple Game",
        renderer="forward",
        backend_name="dx12",
    )

    # Добавляем направленный свет
    light = DirectionalLight(
        direction=Vec3(0.0, -1.0, -0.5),
        color=Vec3(1.0, 1.0, 1.0),
        intensity=1.0
    )
    engine.scene.add_child(light)
    logger.info("[Main] Directional light added")

    # Создаём куб
    cube_mesh = make_cube()
    rotating_cube = RotatingCube(cube_mesh, speed=45.0)
    engine.scene.add_child(rotating_cube.mesh)
    logger.info("[Main] Cube added to scene")

    # Настраиваем камеру
    engine.camera.position = Vec3(2.0, 1.5, 3.0)
    engine.camera.rotation = Vec3(-20.0, 45.0, 0.0)
    logger.info(f"[Main] Camera position: {engine.camera.position}")
    logger.info(f"[Main] Camera rotation: {engine.camera.rotation}")

    # Запускаем движок
    logger.info("[Main] Starting main loop...")
    try:
        engine.run()
    except KeyboardInterrupt:
        logger.info("[Main] Interrupted by user")
    except Exception as e:
        logger.error(f"[Main] Error: {e}")
        import traceback
        traceback.print_exc()
    finally:
        engine.shutdown()
        logger.info("[Main] Engine shut down")


if __name__ == "__main__":
    main()