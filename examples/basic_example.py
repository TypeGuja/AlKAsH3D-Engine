import sys
import os
import ctypes
import numpy as np
from typing import List, Tuple
import logging

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))

from alkash3d.engine import Engine
from alkash3d.utils.config import Config
from alkash3d.graphics.renderers.forward_renderer import ForwardRenderer
from alkash3d.graphics.backend.dx12_backend import DX12Backend
from alkash3d.graphics.shader import Shader
from alkash3d.graphics.mesh import Mesh
from alkash3d.graphics.material import Material
from alkash3d.graphics.texture import Texture
from alkash3d.math.vector import Vector3
from alkash3d.math.matrix import Matrix

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("CubeExample")


class Cube:
    """Класс для создания и управления 3D кубом"""

    # Вершины куба (позиция + нормаль + текстурные координаты)
    VERTICES = np.array([
        # Передняя грань (z = 0.5)
        -0.5, -0.5, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0,
        0.5, -0.5, 0.5, 0.0, 0.0, 1.0, 1.0, 0.0,
        0.5, 0.5, 0.5, 0.0, 0.0, 1.0, 1.0, 1.0,
        -0.5, 0.5, 0.5, 0.0, 0.0, 1.0, 0.0, 1.0,

        # Задняя грань (z = -0.5)
        -0.5, -0.5, -0.5, 0.0, 0.0, -1.0, 1.0, 0.0,
        0.5, -0.5, -0.5, 0.0, 0.0, -1.0, 0.0, 0.0,
        0.5, 0.5, -0.5, 0.0, 0.0, -1.0, 0.0, 1.0,
        -0.5, 0.5, -0.5, 0.0, 0.0, -1.0, 1.0, 1.0,

        # Левая грань (x = -0.5)
        -0.5, -0.5, -0.5, -1.0, 0.0, 0.0, 0.0, 0.0,
        -0.5, -0.5, 0.5, -1.0, 0.0, 0.0, 1.0, 0.0,
        -0.5, 0.5, 0.5, -1.0, 0.0, 0.0, 1.0, 1.0,
        -0.5, 0.5, -0.5, -1.0, 0.0, 0.0, 0.0, 1.0,

        # Правая грань (x = 0.5)
        0.5, -0.5, -0.5, 1.0, 0.0, 0.0, 1.0, 0.0,
        0.5, -0.5, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0,
        0.5, 0.5, 0.5, 1.0, 0.0, 0.0, 0.0, 1.0,
        0.5, 0.5, -0.5, 1.0, 0.0, 0.0, 1.0, 1.0,

        # Верхняя грань (y = 0.5)
        -0.5, 0.5, -0.5, 0.0, 1.0, 0.0, 0.0, 1.0,
        0.5, 0.5, -0.5, 0.0, 1.0, 0.0, 1.0, 1.0,
        0.5, 0.5, 0.5, 0.0, 1.0, 0.0, 1.0, 0.0,
        -0.5, 0.5, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0,

        # Нижняя грань (y = -0.5)
        -0.5, -0.5, -0.5, 0.0, -1.0, 0.0, 0.0, 0.0,
        0.5, -0.5, -0.5, 0.0, -1.0, 0.0, 1.0, 0.0,
        0.5, -0.5, 0.5, 0.0, -1.0, 0.0, 1.0, 1.0,
        -0.5, -0.5, 0.5, 0.0, -1.0, 0.0, 0.0, 1.0,
    ], dtype=np.float32)

    # Индексы для треугольников (36 индексов = 12 треугольников)
    INDICES = np.array([
        0, 1, 2, 0, 2, 3,  # перед
        4, 6, 5, 4, 7, 6,  # зад
        8, 9, 10, 8, 10, 11,  # левая
        12, 14, 13, 12, 15, 14,  # правая
        16, 17, 18, 16, 18, 19,  # верх
        20, 22, 21, 20, 23, 22,  # низ
    ], dtype=np.uint32)

    def __init__(self, backend, renderer):
        self.backend = backend
        self.renderer = renderer
        self.mesh = None
        self.material = None
        self.position = Vector3(0, 0, 5)
        self.rotation = Vector3(0, 0, 0)
        self.scale = Vector3(1, 1, 1)

    def create_checker_texture(self):
        """Создание шахматной текстуры для куба"""
        width, height = 64, 64
        data = bytearray(width * height * 4)

        for y in range(height):
            for x in range(width):
                idx = (y * width + x) * 4
                # Шахматный узор
                if ((x // 8) + (y // 8)) % 2 == 0:
                    data[idx:idx + 4] = [255, 100, 100, 255]  # Красный
                else:
                    data[idx:idx + 4] = [100, 100, 255, 255]  # Синий

        texture = Texture(self.backend)
        texture.create_from_data(width, height, bytes(data))
        return texture

    def create_gradient_texture(self):
        """Создание градиентной текстуры"""
        width, height = 64, 64
        data = bytearray(width * height * 4)

        for y in range(height):
            for x in range(width):
                idx = (y * width + x) * 4
                r = int(255 * x / width)
                g = int(255 * y / height)
                b = int(255 * (1.0 - x / width))
                data[idx:idx + 4] = [r, g, b, 255]

        texture = Texture(self.backend)
        texture.create_from_data(width, height, bytes(data))
        return texture

    def create_solid_color_texture(self, r, g, b):
        """Создание одноцветной текстуры"""
        width, height = 2, 2
        data = bytearray(width * height * 4)
        for i in range(0, len(data), 4):
            data[i:i + 4] = [r, g, b, 255]

        texture = Texture(self.backend)
        texture.create_from_data(width, height, bytes(data))
        return texture

    def initialize(self):
        """Инициализация куба"""
        logger.info("Creating cube mesh...")

        # Создаем меш
        self.mesh = Mesh(self.backend)
        self.mesh.create(self.VERTICES, self.INDICES)

        # Создаем материал с текстурой
        logger.info("Creating material...")
        self.material = Material(self.backend, self.renderer)

        # Используем градиентную текстуру
        texture = self.create_gradient_texture()
        self.material.set_texture(texture)

        # Настраиваем параметры материала
        self.material.set_property("color", [1.0, 1.0, 1.0, 1.0])
        self.material.set_property("roughness", 0.5)
        self.material.set_property("metallic", 0.1)

        logger.info("Cube initialized successfully")

    def update(self, delta_time: float):
        """Обновление куба (вращение)"""
        # Вращаем куб
        self.rotation.y += delta_time * 0.5  # Вращение вокруг Y
        self.rotation.x += delta_time * 0.3  # Вращение вокруг X
        self.rotation.z += delta_time * 0.2  # Вращение вокруг Z

    def get_model_matrix(self):
        """Получение матрицы модели"""
        model = Matrix4x4.identity()
        model = model.translate(self.position)
        model = model.rotate_x(self.rotation.x)
        model = model.rotate_y(self.rotation.y)
        model = model.rotate_z(self.rotation.z)
        model = model.scale(self.scale)
        return model

    def render(self):
        """Рендеринг куба"""
        if self.mesh and self.material:
            self.renderer.render_mesh(self.mesh, self.material, self.get_model_matrix())

    def cleanup(self):
        """Очистка ресурсов"""
        if self.mesh:
            self.mesh.cleanup()
        if self.material:
            self.material.cleanup()


class Camera:
    """Простая камера"""

    def __init__(self, width: int, height: int):
        self.position = Vector3(0, 0, -5)
        self.target = Vector3(0, 0, 0)
        self.up = Vector3(0, 1, 0)
        self.fov = 45.0
        self.aspect = width / height
        self.near_plane = 0.1
        self.far_plane = 100.0

    def get_view_matrix(self):
        """Получение видовой матрицы"""
        return Matrix4x4.look_at(self.position, self.target, self.up)

    def get_projection_matrix(self):
        """Получение матрицы проекции"""
        return Matrix4x4.perspective(self.fov, self.aspect, self.near_plane, self.far_plane)


def main():
    logger.info("Starting 3D Cube Example")

    # Создаем конфигурацию
    config = Config()
    config.set('window.width', 1280)
    config.set('window.height', 720)
    config.set('window.title', 'AlKAsH3D - 3D Cube Example')
    config.set('graphics.vsync', True)
    config.set('graphics.clear_color', [0.1, 0.1, 0.2, 1.0])  # Темно-синий цвет фона

    # Создаем backend и renderer
    logger.info("Initializing graphics backend...")
    backend = DX12Backend(config)

    logger.info("Initializing forward renderer...")
    renderer = ForwardRenderer(config, backend)

    # Создаем движок
    logger.info("Creating engine...")
    engine = Engine(config, renderer, backend)

    # Инициализируем
    if not engine.initialize():
        logger.error("Failed to initialize engine")
        return

    logger.info("Engine initialized successfully")

    # Создаем камеру
    camera = Camera(config.get('window.width'), config.get('window.height'))

    # Создаем куб
    cube = Cube(backend, renderer)
    cube.initialize()

    # Создаем второй куб
    cube2 = Cube(backend, renderer)
    cube2.initialize()
    cube2.position = Vector3(2, 1, 5)
    cube2.scale = Vector3(0.7, 0.7, 0.7)

    # Создаем третий куб
    cube3 = Cube(backend, renderer)
    cube3.initialize()
    cube3.position = Vector3(-2, -1, 6)
    cube3.scale = Vector3(0.5, 0.5, 0.5)

    # Переменные для времени
    import time
    last_time = time.time()
    frame_count = 0

    logger.info("Starting render loop...")

    try:
        while True:
            # Вычисляем delta time
            current_time = time.time()
            delta_time = current_time - last_time
            last_time = current_time

            # Начинаем кадр
            if not engine.begin_frame():
                logger.error("begin_frame failed")
                break

            # Обновляем кубы
            cube.update(delta_time)
            cube2.update(delta_time)
            cube3.update(delta_time)

            # Устанавливаем камеру
            renderer.set_camera(camera)

            # Рендерим кубы
            cube.render()
            cube2.render()
            cube3.render()

            # Заканчиваем кадр
            if not engine.end_frame():
                logger.error("end_frame failed")
                break

            # Считаем FPS
            frame_count += 1
            if frame_count % 60 == 0:
                fps = 1.0 / delta_time if delta_time > 0 else 0
                logger.info(f"FPS: {fps:.1f}")

    except KeyboardInterrupt:
        logger.info("Interrupted by user")
    except Exception as e:
        logger.error(f"Error in render loop: {e}")
        import traceback
        traceback.print_exc()
    finally:
        # Очистка
        logger.info("Cleaning up...")
        cube.cleanup()
        cube2.cleanup()
        cube3.cleanup()
        engine.shutdown()
        logger.info("Done")


if __name__ == "__main__":
    main()