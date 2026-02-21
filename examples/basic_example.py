#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
🎮 AlKAsH3D Engine - Простая тестовая игра (исправленная версия)
"""

import numpy as np
import time
from alkash3d.engine import Engine
from alkash3d.scene import Mesh
from alkash3d.utils import logger


def create_simple_cube():
    """Создаёт простой куб."""
    vertices = np.array([
        # Front face
        -0.5, -0.5, 0.5,
        0.5, -0.5, 0.5,
        0.5, 0.5, 0.5,
        -0.5, 0.5, 0.5,

        # Back face
        -0.5, -0.5, -0.5,
        0.5, -0.5, -0.5,
        0.5, 0.5, -0.5,
        -0.5, 0.5, -0.5,

        # Left face
        -0.5, -0.5, -0.5,
        -0.5, -0.5, 0.5,
        -0.5, 0.5, 0.5,
        -0.5, 0.5, -0.5,

        # Right face
        0.5, -0.5, -0.5,
        0.5, -0.5, 0.5,
        0.5, 0.5, 0.5,
        0.5, 0.5, -0.5,

        # Top face
        -0.5, 0.5, -0.5,
        0.5, 0.5, -0.5,
        0.5, 0.5, 0.5,
        -0.5, 0.5, 0.5,

        # Bottom face
        -0.5, -0.5, -0.5,
        0.5, -0.5, -0.5,
        0.5, -0.5, 0.5,
        -0.5, -0.5, 0.5,
    ], dtype=np.float32).reshape(-1, 3)

    indices = np.array([
        0, 1, 2, 0, 2, 3,  # Front
        4, 6, 5, 4, 7, 6,  # Back
        8, 9, 10, 8, 10, 11,  # Left
        12, 14, 13, 12, 15, 14,  # Right
        16, 18, 17, 16, 19, 18,  # Top
        20, 21, 22, 20, 22, 23,  # Bottom
    ], dtype=np.uint32)

    return Mesh(vertices, indices=indices)


def main():
    """Точка входа."""
    logger.info("=" * 70)
    logger.info("🎮 AlKAsH3D Engine - Simple Test Game")
    logger.info("=" * 70)
    logger.info("Controls:")
    logger.info("  W/A/S/D    - Move forward/left/back/right")
    logger.info("  Space      - Move up")
    logger.info("  Ctrl       - Move down")
    logger.info("  Mouse      - Look around")
    logger.info("  ESC        - Close window")
    logger.info("=" * 70)

    try:
        # Инициализируем движок
        logger.info("Initializing engine...")
        engine = Engine(
            width=1280,
            height=720,
            title="AlKAsH3D - Cube Test",
            renderer="forward",
            backend_name="dx12"
        )

        # Создаём простой куб
        logger.info("Creating cube mesh...")
        cube = create_simple_cube()
        cube.position = np.array([0, 0, 0], dtype=np.float32)
        cube.color = (1, 0, 0)  # Красный

        # Добавляем куб в сцену
        engine.scene.add_child(cube)
        logger.info("Scene ready!")

        # Позиционируем камеру
        engine.camera.position = np.array([0, 0, 3], dtype=np.float32)

        # Главный цикл
        logger.info("Starting game loop...")
        frame_count = 0
        start_time = time.time()
        rotation_angle = 0.0

        while not engine.window.should_close():
            dt = engine.timer.tick()

            # Вращаем куб
            rotation_angle += dt * 1.0  # 1 рад/сек
            try:
                # Попытаемся установить угол вращения разными способами
                if hasattr(cube, 'rotation'):
                    cube.rotation.x = rotation_angle
                    cube.rotation.y = rotation_angle * 0.7
                    cube.rotation.z = rotation_angle * 0.5
                elif hasattr(cube, '_rotation'):
                    cube._rotation[0] = rotation_angle
                    cube._rotation[1] = rotation_angle * 0.7
                    cube._rotation[2] = rotation_angle * 0.5
                else:
                    logger.debug("Cube rotation attribute not available")
            except Exception as e:
                logger.debug(f"Could not set rotation: {e}")

            # Обновляем события окна
            engine.window.poll_events()

            # Обновляем камеру
            engine.camera.update_fly(dt, engine.window.input)

            # Обновляем сцену
            engine.scene.update(dt)

            # ✅ КРИТИЧНО: Рендерим
            engine.renderer.render(engine.scene, engine.camera)

            # ✅ КРИТИЧНО: Обновляем экран
            engine.window.swap_buffers()

            # Статистика
            frame_count += 1
            elapsed = time.time() - start_time
            if elapsed > 0 and frame_count % 60 == 0:
                fps = frame_count / elapsed
                logger.info(f"FPS: {fps:.1f}, Frames: {frame_count}")

        logger.info("Game loop ended")

    except KeyboardInterrupt:
        logger.info("Interrupted by user")
    except Exception as e:
        logger.error(f"Fatal error: {e}")
        import traceback
        traceback.print_exc()
    finally:
        try:
            engine.shutdown()
            logger.info("Engine shutdown complete")
        except:
            pass


if __name__ == "__main__":
    main()