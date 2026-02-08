import alkash3d as ak
from alkash3d.utils import logger
import numpy as np


def create_simple_cube():
    """Минимальный куб для отладки"""
    vertices = np.array([
        -0.5, -0.5, 0.5,
        0.5, -0.5, 0.5,
        0.5, 0.5, 0.5,
        -0.5, 0.5, 0.5,
        -0.5, -0.5, -0.5,
        0.5, -0.5, -0.5,
        0.5, 0.5, -0.5,
        -0.5, 0.5, -0.5,
    ], dtype=np.float32)

    indices = np.array([
        0, 1, 2, 0, 2, 3,  # Front
        4, 6, 5, 4, 7, 6,  # Back
        0, 4, 5, 0, 5, 1,  # Bottom
        2, 6, 7, 2, 7, 3,  # Top
        0, 3, 7, 0, 7, 4,  # Left
        1, 5, 6, 1, 6, 2,  # Right
    ], dtype=np.uint32)

    normals = np.array([
        0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1,
        0, 0, -1, 0, 0, -1, 0, 0, -1, 0, 0, -1,
    ], dtype=np.float32)

    # Простые UV координаты
    texcoords = np.tile(np.array([[0, 0], [1, 0], [1, 1], [0, 1]], dtype=np.float32), (2, 1))

    return ak.Mesh(vertices=vertices, normals=normals, indices=indices, texcoords=texcoords, name="Cube")


if __name__ == "__main__":
    logger.info("Starting minimal example...")

    engine = ak.Engine(width=800, height=600, title="Minimal Test", renderer="forward")

    # Добавляем куб
    cube = create_simple_cube()
    engine.scene.add_child(cube)

    # Настраиваем камеру
    engine.camera.position = ak.Vec3(0, 0, 2)

    logger.info("Running engine...")
    engine.run()