# examples/basic_example.py
import alkash3d as ak
import numpy as np


def create_cube():
    """Создает простой куб"""
    vertices = np.array([
        # Передняя грань
        -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0,
        -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0,
        # Задняя грань
        -1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0,
        -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0,
        # Верхняя грань
        -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
        -1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, -1.0,
        # Правая грань
        1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0,
        1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0,
        # Левая грань
        -1.0, -1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0,
        -1.0, -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0,
    ], dtype=np.float32)

    return ak.Mesh(vertices=vertices, name="Cube")


class Game:
    def __init__(self):
        # Создаем движок
        self.engine = ak.Engine(
            width=1280,
            height=720,
            title="AlKAsH3D Game Example",
            renderer="forward"
        )

        # Создаем игровые объекты
        self.setup_scene()

    def setup_scene(self):
        # Добавляем куб
        self.cube = create_cube()
        self.cube.position = ak.Vec3(0, 0, 0)
        self.engine.scene.add_child(self.cube)

        # Добавляем свет
        light = ak.DirectionalLight(
            direction=ak.Vec3(-1, -1, -1),
            color=ak.Vec3(1, 1, 1),
            intensity=1.0
        )
        self.engine.scene.add_child(light)

        # Настройка камеры
        self.engine.camera.position = ak.Vec3(0, 0, 5)

    def run(self):
        self.engine.run()


if __name__ == "__main__":
    game = Game()
    game.run()
