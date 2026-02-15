# ==============================================================
# examples/rtx_demo.py
# --------------------------------------------------------------
# Мини‑демо‑сцена, использующая режим "rtx" (Rust‑RT‑pipeline)
# ==============================================================
import numpy as np
import glfw
from pathlib import Path

import alkash3d as ak
from alkash3d import (
    Engine,
    Camera,
    DirectionalLight,
    Mesh,
    Node,
    Vec3,
)

# -----------------------------------------------------------------
# Утилита: простая геометрия куба (1.txt м³, центр в (0,0,0))
# -----------------------------------------------------------------
def make_box_mesh(name: str = "Box") -> Mesh:
    # 8 вершин (x, y, z)
    verts = np.array(
        [
            -0.5, -0.5, -0.5,   # 0
            +0.5, -0.5, -0.5,   # 1.txt
            +0.5, +0.5, -0.5,   # 2
            -0.5, +0.5, -0.5,   # 3
            -0.5, -0.5, +0.5,   # 4
            +0.5, -0.5, +0.5,   # 5
            +0.5, +0.5, +0.5,   # 6
            -0.5, +0.5, +0.5,   # 7
        ],
        dtype=np.float32,
    )

    # индексы (2 треугольника на грань)
    inds = np.array(
        [
            0, 1, 2, 0, 2, 3,   # back
            4, 5, 6, 4, 6, 7,   # front
            0, 4, 7, 0, 7, 3,   # left
            1, 5, 6, 1, 6, 2,   # right
            3, 2, 6, 3, 6, 7,   # top
            0, 1, 5, 0, 5, 4,   # bottom
        ],
        dtype=np.uint32,
    )

    return Mesh(vertices=verts, indices=inds, name=name)


# -----------------------------------------------------------------
# Утилита: простая диск‑сфера (используем только для визуального контроля)
# -----------------------------------------------------------------
def make_sphere_mesh(radius: float = 1.0,
                     stacks: int = 16,
                     slices: int = 16,
                     name: str = "Sphere") -> Mesh:
    """Генерирует UV‑сферу (только позиции + нормали)."""
    positions = []
    normals = []

    for i in range(stacks + 1):
        lat = np.pi / 2 - i * np.pi / stacks      # от +π/2 до -π/2
        sin_lat = np.sin(lat)
        cos_lat = np.cos(lat)

        for j in range(slices + 1):
            lon = 2 * np.pi * j / slices
            sin_lon = np.sin(lon)
            cos_lon = np.cos(lon)

            x = cos_lat * cos_lon
            y = sin_lat
            z = cos_lat * sin_lon

            positions.extend([radius * x, radius * y, radius * z])
            normals.extend([x, y, z])   # нормаль = единичный вектор

    # индексы – triangle strip
    indices = []
    for i in range(stacks):
        for j in range(slices):
            first = i * (slices + 1) + j
            second = first + slices + 1
            indices.extend([first, second, first + 1])
            indices.extend([second, second + 1, first + 1])

    positions = np.array(positions, dtype=np.float32)
    normals   = np.array(normals,   dtype=np.float32)
    indices   = np.array(indices,   dtype=np.uint32)

    # В AlKAsH3D у Mesh‑а есть поле `normals`; передаём их
    return Mesh(vertices=positions, normals=normals, indices=indices, name=name)


# -----------------------------------------------------------------
# 1️⃣  Создаём движок с RTX‑режимом
# -----------------------------------------------------------------
engine = Engine(
    width=1280,
    height=720,
    title="AlKAsH3D – RTX‑demo (Rust‑CPU‑tracer)",
    renderer="rtx",          # ← ключевой параметр
)

# -----------------------------------------------------------------
# 2️⃣  Сцена: пол и несколько объектов
# -----------------------------------------------------------------
scene = engine.scene   # уже созданный на этапе Engine.__init__

# --- пол (плоскость XZ размером 20×20, позиция y = 0 ---
plane_vertices = np.array(
    [
        -10.0, 0.0, -10.0,
        +10.0, 0.0, -10.0,
        +10.0, 0.0, +10.0,
        -10.0, 0.0, +10.0,
    ],
    dtype=np.float32,
)

plane_normals = np.tile([0.0, 1.0, 0.0], 4)   # нормаль вверх
plane_indices = np.array([0, 1, 2, 0, 2, 3], dtype=np.uint32)

plane = Mesh(
    vertices=plane_vertices,
    normals=plane_normals,
    indices=plane_indices,
    name="Ground",
)
plane.position = ak.Vec3(0.0, 0.0, 0.0)
scene.add_child(plane)

# --- Объекты, задаём им разные цвета -----------------
def add_colored_box(pos: ak.Vec3, scale: ak.Vec3, color: ak.Vec3, name: str):
    """Создаёт куб‑объект, задаёт позицию, масштаб и базовый цвет."""
    box = make_box_mesh(name)
    box.position = pos
    box.scale = scale
    box.color = color          # ← именно этот цвет будет использован в CPU‑tracer'е
    scene.add_child(box)

# Красный куб (будет определять цвет сферы, т.к. первый в списке)
add_colored_box(
    pos=ak.Vec3(-2.0, 0.5, -2.0),
    scale=ak.Vec3(1.0, 1.0, 1.0),
    color=ak.Vec3(1.0, 0.2, 0.2),   # ярко‑красный
    name="RedBox",
)

# Зеленый куб (последующий – просто для визуального контроля)
add_colored_box(
    pos=ak.Vec3(2.0, 0.5, 1.5),
    scale=ak.Vec3(1.5, 1.5, 1.5),
    color=ak.Vec3(0.2, 1.0, 0.2),   # зелёный
    name="GreenBox",
)

# Синяя сфера (только для иллюзии, в заглушке она не используется)
sphere = make_sphere_mesh(radius=0.6, stacks=16, slices=16, name="BlueSphere")
sphere.position = ak.Vec3(0.0, 0.6, 3.0)
sphere.color = ak.Vec3(0.2, 0.2, 1.0)  # синий
scene.add_child(sphere)

# -----------------------------------------------------------------
# 3️⃣  Освещение (не используется в текущей CPU‑tracer‑заглушке,
#     но оставляем для совместимости с другими пайплайнами)
# -----------------------------------------------------------------
sun = DirectionalLight(
    direction=ak.Vec3(-0.5, -1.0, -0.5).normalized(),
    color=ak.Vec3(1.0, 0.95, 0.85),
    intensity=2.0,
    name="Sun",
)
scene.add_child(sun)

# -----------------------------------------------------------------
# 4️⃣  Камера (пример fly‑camera)
# -----------------------------------------------------------------
engine.camera.position = ak.Vec3(0.0, 1.6, 8.0)   # стартовая позиция
engine.camera.rotation.x = -15.0                # слегка наклонена вниз

# -----------------------------------------------------------------
# 5️⃣  Переходим к запуску
# -----------------------------------------------------------------
ak.logger.info("[RTDemo] Запуск сцены с RTX‑режимом (Rust‑CPU‑tracer)")
engine.run()
ak.logger.info("[RTDemo] Демо завершено")
