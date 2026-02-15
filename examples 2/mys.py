#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Максимально простой пример - красный куб с минимальными шейдерами.
ПРИНУДИТЕЛЬНОЕ использование простых шейдеров.
"""

from __future__ import annotations

import numpy as np
import os
import sys
from pathlib import Path

# Добавляем путь к проекту
sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d import (
    Engine,
    Scene,
    Camera,
    Mesh,
    Vec3,
)
from alkash3d.utils import logger

# ПРИНУДИТЕЛЬНО создаем простые шейдеры
SIMPLE_VERT = """float4 VSMain(float4 pos : POSITION) : SV_POSITION {
    return pos;
}
"""

SIMPLE_FRAG = """float4 PSMain() : SV_TARGET {
    return float4(1.0, 0.0, 0.0, 1.0);
}
"""

# Создаем файлы шейдеров
shader_dir = Path("resources/shaders")
shader_dir.mkdir(parents=True, exist_ok=True)

vert_path = shader_dir / "forward_vert.hlsl"
frag_path = shader_dir / "forward_frag.hlsl"

with open(vert_path, "w") as f:
    f.write(SIMPLE_VERT)
print(f"✅ Created {vert_path}")

with open(frag_path, "w") as f:
    f.write(SIMPLE_FRAG)
print(f"✅ Created {frag_path}")

# Теперь Monkey Patch - заменяем функцию компиляции шейдеров в движке
from alkash3d.graphics.utils import d3d12_wrapper as dx

original_compile = dx.compile_shader


def patched_compile_shader(path, entry, profile):
    """Перехватываем компиляцию и используем наши простые шейдеры."""
    print(f"\n[DEBUG] patched_compile_shader called with: {path}")
    print(f"[DEBUG] entry: {entry}, profile: {profile}")

    # Если запрашивают forward_vert, подменяем на simple_vert
    if "forward_vert" in str(path):
        new_path = str(shader_dir / "simple_vert.hlsl")
        print(f"[DEBUG] Redirecting to: {new_path}")
        return original_compile(new_path, "VSMain", "vs_5_0")

    # Если запрашивают forward_frag, подменяем на simple_frag
    elif "forward_frag" in str(path):
        new_path = str(shader_dir / "simple_frag.hlsl")
        print(f"[DEBUG] Redirecting to: {new_path}")
        return original_compile(new_path, "PSMain", "ps_5_0")

    # Иначе используем оригинальную функцию
    return original_compile(path, entry, profile)


# Подменяем функцию
dx.compile_shader = patched_compile_shader
print("✅ Patched shader compilation")


def create_cube_vertices() -> np.ndarray:
    """Создает вершины куба."""
    return np.array([
        [-0.5, -0.5, -0.5, 1.0],
        [0.5, -0.5, -0.5, 1.0],
        [0.5, 0.5, -0.5, 1.0],
        [-0.5, 0.5, -0.5, 1.0],
        [-0.5, -0.5, 0.5, 1.0],
        [0.5, -0.5, 0.5, 1.0],
        [0.5, 0.5, 0.5, 1.0],
        [-0.5, 0.5, 0.5, 1.0],
    ], dtype=np.float32)


def create_cube_indices() -> np.ndarray:
    """Создает индексы для куба."""
    return np.array([
        0, 1, 2, 0, 2, 3,
        4, 5, 6, 4, 6, 7,
        1, 5, 6, 1, 6, 2,
        0, 4, 7, 0, 7, 3,
        3, 2, 6, 3, 6, 7,
        0, 1, 5, 0, 5, 4,
    ], dtype=np.uint32)


class SimpleCube(Mesh):
    """Простой куб."""

    def __init__(self):
        vertices = create_cube_vertices()
        indices = create_cube_indices()

        super().__init__(
            vertices=vertices[:, :3],
            indices=indices,
            name="SimpleCube"
        )

        self.position = Vec3(0, 0, 0)
        self.rotation = Vec3(0, 0, 0)
        self.scale = Vec3(1, 1, 1)
        print(f"[DEBUG] Cube created at {self.position}")

    def on_update(self, dt: float) -> None:
        self.rotation.y += 45.0 * dt


def main():
    print("\n" + "=" * 60)
    print("SIMPLE CUBE DEMO - WITH PATCHED SHADERS")
    print("=" * 60 + "\n")

    # Создаем движок
    engine = Engine(
        width=800,
        height=600,
        title="SIMPLE CUBE",
        renderer="forward",
        backend_name="dx12",
    )

    # Создаем сцену
    scene = Scene()

    # Камера
    camera = Camera(fov=70.0)
    camera.position = Vec3(2, 1.5, 3)
    camera.rotation = Vec3(-15, 0, 0)
    scene.add_child(camera)
    scene.camera = camera
    print(f"[DEBUG] Camera at {camera.position}")

    # Куб
    cube = SimpleCube()
    scene.add_child(cube)

    engine.scene = scene

    print("\n" + "=" * 60)
    print("Engine started - SHOULD SEE RED CUBE!")
    print("=" * 60 + "\n")

    try:
        engine.run()
    except Exception as e:
        logger.error(f"Error: {e}")
        import traceback
        traceback.print_exc()
    finally:
        print("\n" + "=" * 60)
        print("Demo finished")
        print("=" * 60 + "\n")


if __name__ == "__main__":
    main()