#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
🎮 AlKAsH3D Engine - Simple Test Game
ФИНАЛЬНАЯ РАБОЧАЯ ВЕРСИЯ
"""

import numpy as np
import time
import sys
from alkash3d.engine import Engine
from alkash3d.scene import Mesh
from alkash3d.utils import logger


def create_simple_cube():
    """Создаёт простой куб."""
    vertices = np.array([
        # Front
        -0.5, -0.5, 0.5, 0.5, -0.5, 0.5, 0.5, 0.5, 0.5, -0.5, 0.5, 0.5,
        # Back
        -0.5, -0.5, -0.5, 0.5, -0.5, -0.5, 0.5, 0.5, -0.5, -0.5, 0.5, -0.5,
        # Left
        -0.5, -0.5, -0.5, -0.5, -0.5, 0.5, -0.5, 0.5, 0.5, -0.5, 0.5, -0.5,
        # Right
        0.5, -0.5, -0.5, 0.5, -0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, -0.5,
        # Top
        -0.5, 0.5, -0.5, 0.5, 0.5, -0.5, 0.5, 0.5, 0.5, -0.5, 0.5, 0.5,
        # Bottom
        -0.5, -0.5, -0.5, 0.5, -0.5, -0.5, 0.5, -0.5, 0.5, -0.5, -0.5, 0.5,
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
    logger.info("  W/A/S/D    - Move (forward/left/back/right)")
    logger.info("  Space      - Move up")
    logger.info("  Ctrl       - Move down")
    logger.info("  Mouse      - Look around")
    logger.info("  ESC        - Close window")
    logger.info("=" * 70)

    engine = None

    try:
        # Инициализируем движок
        logger.info("Initializing engine...")
        print("DEBUG: About to create Engine", flush=True)

        engine = Engine(
            width=1280,
            height=720,
            title="AlKAsH3D - Cube Test",
            renderer="forward",
            backend_name="dx12"
        )

        print(f"DEBUG: Engine created: {engine}", flush=True)

        # �� ПРОВЕРКА: Engine создан?
        if engine is None:
            logger.error("Failed to create engine (engine is None)")
            return 1

        logger.info(f"✅ Engine created successfully!")
        logger.info(f"   Window: {engine.window.width}x{engine.window.height}")
        logger.info(f"   Backend: {engine.backend.__class__.__name__}")
        logger.info(f"   Renderer: {engine.renderer.__class__.__name__}")

        # Создаём куб
        logger.info("Creating cube mesh...")
        print("DEBUG: About to create cube", flush=True)

        cube = create_simple_cube()
        cube.position = np.array([0, 0, 0], dtype=np.float32)
        cube.color = (1, 0, 0)

        print("DEBUG: Cube created, adding to scene", flush=True)

        # Добавляем куб в сцену
        engine.scene.add_child(cube)
        logger.info("✅ Cube added to scene")

        # Позиционируем камеру
        engine.camera.position = np.array([0, 0, 3], dtype=np.float32)
        logger.info(f"✅ Camera position set: {engine.camera.position}")

        logger.info("✅ Scene ready!")
        logger.info("=" * 70)
        logger.info("Starting game loop...")
        logger.info("=" * 70)

        # ✅ ОТЛАДКА: Проверяем окно перед циклом
        print("DEBUG: Before window checks", flush=True)

        should_close = engine.window.should_close()
        logger.info(f"Window should_close: {should_close}")
        logger.info(f"Window handle: {engine.window.handle}")
        logger.info(f"Window size: {engine.window.width}x{engine.window.height}")

        print(f"DEBUG: should_close = {should_close}", flush=True)

        logger.info("=" * 70)
        logger.info("🎮 ENTERING MAIN GAME LOOP 🎮")
        logger.info("=" * 70)

        print("DEBUG: About to enter while loop", flush=True)
        sys.stdout.flush()

        # ✅ ГЛАВНЫЙ ЦИКЛ
        frame_count = 0
        start_time = time.time()
        rotation_angle = 0.0
        last_log_time = time.time()

        loop_iteration = 0

        while not engine.window.should_close():
            loop_iteration += 1
            if loop_iteration == 1:
                print("DEBUG: FIRST ITERATION OF MAIN LOOP!", flush=True)
                logger.info("✅ FIRST ITERATION OF MAIN LOOP!")

            # Получаем delta time
            dt = engine.timer.tick()

            # Вращаем куб
            rotation_angle += dt * 1.0
            try:
                if hasattr(cube, 'rotation'):
                    cube.rotation.x = rotation_angle
                    cube.rotation.y = rotation_angle * 0.7
                    cube.rotation.z = rotation_angle * 0.5
            except Exception as e:
                pass

            # Обновляем события окна
            try:
                engine.window.poll_events()
            except Exception as e:
                logger.debug(f"poll_events error: {e}")
                break

            # Обновляем камеру (WASD + мышь)
            try:
                engine.camera.update_fly(dt, engine.window.input)
            except Exception as e:
                logger.debug(f"Camera update error: {e}")

            # Обновляем сцену
            try:
                engine.scene.update(dt)
            except Exception as e:
                logger.debug(f"Scene update error: {e}")

            # ✅ РЕНДЕРИМ
            try:
                engine.renderer.render(engine.scene, engine.camera)
            except Exception as e:
                logger.debug(f"Render error: {e}")

            # ✅ ОБНОВЛЯЕМ ЭКРАН (самое важное!)
            try:
                engine.window.swap_buffers()
            except Exception as e:
                logger.debug(f"swap_buffers error: {e}")

            # Логирование FPS каждые 2 секунды
            frame_count += 1
            current_time = time.time()
            elapsed = current_time - start_time

            if current_time - last_log_time >= 2.0:
                if elapsed > 0:
                    fps = frame_count / elapsed
                    logger.info(f"✅ FPS: {fps:.1f} | Frames: {frame_count} | Time: {elapsed:.1f}s")
                last_log_time = current_time

        logger.info("=" * 70)
        logger.info("✅ Game loop ended")
        logger.info(f"Total frames: {frame_count}")
        logger.info(f"Total time: {time.time() - start_time:.2f}s")
        logger.info("=" * 70)
        return 0

    except KeyboardInterrupt:
        logger.info("Game interrupted by user (Ctrl+C)")
        return 0
    except Exception as e:
        logger.error(f"❌ Fatal error: {e}")
        print(f"DEBUG: Exception: {e}", flush=True)
        import traceback
        traceback.print_exc()
        return 1
    finally:
        # Очистка
        if engine is not None:
            try:
                logger.info("Shutting down engine...")
                engine.shutdown()
                logger.info("✅ Engine shutdown complete")
            except Exception as e:
                logger.error(f"Shutdown error: {e}")

        logger.info("Program finished")


if __name__ == "__main__":
    print("DEBUG: Script started", flush=True)
    exit_code = main()
    print(f"DEBUG: Script exiting with code {exit_code}", flush=True)
    sys.exit(exit_code)