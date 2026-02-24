# test_minimal.py
"""
Минимальный тест без плагинов
"""

from alkash3d import Engine, Scene, Mesh, Vec3
import numpy as np


def main():
    # Создаем движок без плагинов
    print("Создаем движок...")
    engine = Engine(
        width=800,
        height=600,
        title="Minimal Test",
        renderer="forward"
    )

    # Отключаем плагины временно
    engine.plugin_manager = None

    # Создаем простой куб
    print("Создаем куб...")
    vertices = np.array([
        [-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5],
        [-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5],
    ], dtype=np.float32)

    indices = np.array([
        0, 1, 2, 0, 2, 3, 1, 5, 6, 1, 6, 2, 5, 4, 7, 5, 7, 6, 4, 0, 3, 4, 3, 7, 3, 2, 6, 3, 6, 7, 4, 5, 1, 4, 1, 0
    ], dtype=np.uint32)

    cube = Mesh(vertices, indices, name="TestCube")
    cube.position = Vec3(0, 0, 0)
    engine.scene.add_child(cube)

    # Камера
    engine.camera.position = Vec3(2, 2, 5)

    # Запуск
    print("Запуск... Нажмите ESC для выхода")

    import time
    last_time = time.time()

    while not engine.window.should_close():
        dt = time.time() - last_time
        last_time = time.time()

        # Вращаем куб
        cube.rotation.y += 50.0 * dt

        # Рендеринг
        engine.renderer.render(engine.scene, engine.camera)

    engine.shutdown()


if __name__ == "__main__":
    main()