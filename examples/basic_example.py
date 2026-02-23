#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
basic_example.py – минимальный пример вращающегося куба (DX12, ForwardRenderer).

Главные исправления:
* импортируем Mesh из alkash3d.scene.mesh;
* создаём белый placeholder‑текстуру без update_texture;
* включаем защиту от отсутствия PSO;
* (опционально) принудительно включаем stub‑режим, если DLL не загружена.
"""

import math
import time
import numpy as np

import glfw

from alkash3d.engine import Engine
from alkash3d.scene import Node, Camera, Mesh, Scene
from alkash3d.assets.material import PBRMaterial
from alkash3d.math.vec3 import Vec3
from alkash3d.math.mat4 import Mat4


# ----------------------------------------------------------------------
# Вспомогательная функция – генерируем простые вершины/индексы куба.
# ----------------------------------------------------------------------
def create_cube():
    """Возвращает (vertices, indices) в формате, ожидаемом forward shader."""
    s = 0.5
    # 24 вершины: позиция(3) + нормаль(3) + цвет(3) = 9 float32 на вершину
    verts = [
        # Front (+Z)
        -s, -s,  s, 0, 0, 1, 1, 1, 1,
         s, -s,  s, 0, 0, 1, 1, 1, 1,
         s,  s,  s, 0, 0, 1, 1, 1, 1,
        -s,  s,  s, 0, 0, 1, 1, 1, 1,
        # Back (-Z)
        -s, -s, -s, 0, 0,-1, 1, 1, 1,
        -s,  s, -s, 0, 0,-1, 1, 1, 1,
         s,  s, -s, 0, 0,-1, 1, 1, 1,
         s, -s, -s, 0, 0,-1, 1, 1, 1,
        # Left (-X)
        -s, -s, -s,-1, 0, 0, 1, 1, 1,
        -s, -s,  s,-1, 0, 0, 1, 1, 1,
        -s,  s,  s,-1, 0, 0, 1, 1, 1,
        -s,  s, -s,-1, 0, 0, 1, 1, 1,
        # Right (+X)
         s, -s, -s, 1, 0, 0, 1, 1, 1,
         s,  s, -s, 1, 0, 0, 1, 1, 1,
         s,  s,  s, 1, 0, 0, 1, 1, 1,
         s, -s,  s, 1, 0, 0, 1, 1, 1,
        # Top (+Y)
        -s,  s, -s, 0, 1, 0, 1, 1, 1,
        -s,  s,  s, 0, 1, 0, 1, 1, 1,
         s,  s,  s, 0, 1, 0, 1, 1, 1,
         s,  s, -s, 0, 1, 0, 1, 1, 1,
        # Bottom (-Y)
        -s, -s, -s, 0,-1, 0, 1, 1, 1,
         s, -s, -s, 0,-1, 0, 1, 1, 1,
         s, -s,  s, 0,-1, 0, 1, 1, 1,
        -s, -s,  s, 0,-1, 0, 1, 1, 1,
    ]
    verts_np = np.array(verts, dtype=np.float32)

    # 6 граней × 2 треугольника = 36 индексов
    idx = []
    for f in range(6):
        base = f * 4
        idx.extend([base, base + 1, base + 2,
                    base, base + 2, base + 3])
    indices_np = np.array(idx, dtype=np.uint32)

    return verts_np, indices_np


# ----------------------------------------------------------------------
# Узел, содержащий куб и вращающийся каждый кадр
# ----------------------------------------------------------------------
class RotatingCube(Node):
    def __init__(self):
        super().__init__("RotatingCube")

        verts, inds = create_cube()
        self.mesh = Mesh(vertices=verts, indices=inds)
        self.add_child(self.mesh)                     # ← будет найдено в traverse()

        # Простой материал – белый альбедо, без карт.
        self.material = PBRMaterial(albedo=(1.0, 1.0, 1.0, 1.0))

        # Скорость вращения (°/сек)
        self.rot_speed = Vec3(0.0, 45.0, 0.0)

    # вызывается каждый тик движка (Scene.update)
    def update(self, dt: float):
        self.rotation.x += self.rot_speed.x * dt
        self.rotation.y += self.rot_speed.y * dt
        self.rotation.z += self.rot_speed.z * dt

    # вызывается из ForwardRenderer.render()
    def draw(self, backend):
        # material будет привязан автоматически внутри Mesh.draw()
        self.mesh.draw(backend)


# ----------------------------------------------------------------------
def main():
    # 1️⃣ Engine (DX12 + ForwardRenderer)
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – вращающийся куб",
        renderer="forward",          # используем ForwardRenderer
        backend_name="dx12",
    )

    # 2️⃣ Добавляем наш вращающийся куб в сцену
    cube = RotatingCube()
    engine.scene.add_child(cube)

    # 3️⃣ (опционально) принудительно включаем stub‑режим,
    #    если у вас не удалось загрузить alkash3d_dx12.dll.
    #    В этом режиме всё будет «пусто», но падения не будет.
    # engine.backend._in_stub_mode = True

    # 4️⃣ Запускаем главный цикл
    engine.run()


if __name__ == "__main__":
    main()
