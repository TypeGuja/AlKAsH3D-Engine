"""
AlKAsH3D - ФИНАЛЬНЫЙ ТЕСТ с OpenGL
Сохраните как final_opengl_test.py
"""

import numpy as np
from alkash3d.engine import Engine
from alkash3d.scene import Camera, Mesh

def create_cube():
    """Создание простого куба"""
    vertices = np.array([
        [-1, -1, -1], [ 1, -1, -1], [ 1,  1, -1], [-1,  1, -1],
        [-1, -1,  1], [ 1, -1,  1], [ 1,  1,  1], [-1,  1,  1]
    ], dtype=np.float32)

    indices = np.array([
        0,1,2, 0,2,3, 4,6,5, 4,7,6,
        0,3,7, 0,7,4, 1,5,6, 1,6,2,
        0,4,5, 0,5,1, 3,2,6, 3,6,7
    ], dtype=np.uint32)

    return vertices, None, None, indices

def main():
    print("="*60)
    print("AlKAsH3D - ФИНАЛЬНЫЙ ТЕСТ с OpenGL")
    print("="*60)

    # Пробуем разные варианты создания Engine
    try:
        # Вариант 1: OpenGL бэкенд
        print("\n🔵 Попытка 1: OpenGL бэкенд")
        engine = Engine(
            width=1024,
            height=768,
            title="OpenGL Test",
            backend_name="opengl"  # или "gl"
        )
    except Exception as e:
        print(f"❌ Ошибка: {e}")

        try:
            # Вариант 2: OpenGL без параметров
            print("\n🔵 Попытка 2: OpenGL без параметров")
            engine = Engine(backend_name="opengl")
        except Exception as e:
            print(f"❌ Ошибка: {e}")

            try:
                # Вариант 3: Простой Engine (пусть сам выбирает)
                print("\n🔵 Попытка 3: Автовыбор")
                engine = Engine()
            except Exception as e:
                print(f"❌ Все варианты провалились: {e}")
                return

    # Получаем сцену
    scene = engine.scene

    # Создаем камеру
    camera = Camera()
    camera.position = np.array([3, 2, 5], dtype=np.float32)
    scene.add_child(camera)
    engine.camera = camera

    # Создаем куб
    vertices, normals, texcoords, indices = create_cube()
    cube = Mesh(vertices, normals, texcoords, indices)
    cube.position = np.array([0, 0, 0], dtype=np.float32)
    scene.add_child(cube)

    print("\n✅ Все готово! Запуск...")
    print("   Если увидите куб - проблема в DirectX")
    print("   Если нет - проблема в самом движке")
    print("\n" + "="*60)

    # Запуск
    try:
        engine.run()
    except Exception as e:
        print(f"❌ Ошибка при запуске: {e}")
        import traceback
        traceback.print_exc()

if __name__ == "__main__":
    main()