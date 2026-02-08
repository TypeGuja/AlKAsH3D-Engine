import alkash3d as ak
from alkash3d.utils import logger, gl_check_error
import numpy as np


def create_cube():
    """Создаёт куб с вершинами, нормалями И текстурными координатами"""
    # Вершины куба
    vertices = np.array([
        # Front face
        -1, -1, 1, 1, -1, 1, 1, 1, 1, -1, 1, 1,
        # Back face
        -1, -1, -1, -1, 1, -1, 1, 1, -1, 1, -1, -1,
        # Top face
        -1, 1, -1, -1, 1, 1, 1, 1, 1, 1, 1, -1,
        # Bottom face
        -1, -1, -1, 1, -1, -1, 1, -1, 1, -1, -1, 1,
        # Right face
        1, -1, -1, 1, 1, -1, 1, 1, 1, 1, -1, 1,
        # Left face
        -1, -1, -1, -1, -1, 1, -1, 1, 1, -1, 1, -1,
    ], dtype=np.float32).reshape(-1, 3)

    # Индексы
    indices = np.array([
        0, 1, 2, 0, 2, 3,
        4, 6, 5, 4, 7, 6,
        8, 9, 10, 8, 10, 11,
        12, 13, 14, 12, 14, 15,
        16, 17, 18, 16, 18, 19,
        20, 21, 22, 20, 22, 23,
    ], dtype=np.uint32)

    # Нормали
    normals = np.array([
        0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1,
        0, 0, -1, 0, 0, -1, 0, 0, -1, 0, 0, -1,
        0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0,
        0, -1, 0, 0, -1, 0, 0, -1, 0, 0, -1, 0,
        1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0,
        -1, 0, 0, -1, 0, 0, -1, 0, 0, -1, 0, 0,
    ], dtype=np.float32).reshape(-1, 3)

    # ДОБАВЛЯЕМ ТЕКСТУРНЫЕ КООРДИНАТЫ
    texcoords = np.array([
        # Front face
        0, 0, 1, 0, 1, 1, 0, 1,
        # Back face
        0, 0, 1, 0, 1, 1, 0, 1,
        # Top face
        0, 0, 1, 0, 1, 1, 0, 1,
        # Bottom face
        0, 0, 1, 0, 1, 1, 0, 1,
        # Right face
        0, 0, 1, 0, 1, 1, 0, 1,
        # Left face
        0, 0, 1, 0, 1, 1, 0, 1,
    ], dtype=np.float32).reshape(-1, 2)

    mesh = ak.Mesh(
        vertices=vertices,
        normals=normals,
        indices=indices,
        texcoords=texcoords,
        name="Cube"
    )
    return mesh


class Game:
    def __init__(self):
        logger.info("=== Инициализация игры ===")

        self.engine = ak.Engine(
            width=1280,
            height=720,
            title="AlKAsH3D Game Example",
            renderer="forward"
        )

        gl_check_error("После создания Engine")
        logger.info("✓ Движок инициализирован")

        self.setup_scene()

        gl_check_error("После setup_scene")
        logger.info("✓ Сцена подготовлена")

    def setup_scene(self):
        logger.info("Подготовка сцены...")

        logger.info("Создание куба...")
        self.cube = create_cube()
        self.cube.position = ak.Vec3(0, 0, 0)
        self.engine.scene.add_child(self.cube)
        logger.info("✓ Куб добавлен в сцену")
        logger.info(f"✓ Куб имеет текстурные координаты: {self.cube.texcoords is not None}")

        gl_check_error("После добавления куба")

        logger.info("Создание освещения...")
        light = ak.DirectionalLight(
            direction=ak.Vec3(-1, -1, -1),
            color=ak.Vec3(1, 1, 1),
            intensity=1.0
        )
        self.engine.scene.add_child(light)
        logger.info("✓ Свет добавлен в сцену")

        gl_check_error("После добавления света")

        logger.info("Настройка камеры...")
        self.engine.camera.position = ak.Vec3(0, 0, 5)
        logger.info("✓ Камера позиционирована")

        gl_check_error("После настройки камеры")

        objects = list(self.engine.scene.traverse())
        logger.info(f"Объектов в сцене: {len(objects)}")
        for obj in objects:
            logger.info(f"  - {obj.name}")

    def run(self):
        logger.info("=== Запуск основного цикла ===")
        self.engine.run()


if __name__ == "__main__":
    game = Game()
    game.run()