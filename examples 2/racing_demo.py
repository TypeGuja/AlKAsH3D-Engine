#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
Racing demo – вращающий куб.
FPS выводится в консоль (INFO‑уровень).
"""

from __future__ import annotations

import sys
import numpy as np
from pathlib import Path

# -------------------------------------------------
# Добавляем корень проекта в PYTHONPATH
# -------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(PROJECT_ROOT))

# -------------------------------------------------
# Public API импорты
# -------------------------------------------------
from alkash3d import (
    Engine,
    Scene,
    Camera,
    DirectionalLight,
    PointLight,
    Mesh,
    Vec3,
    PBRMaterial,
)
from alkash3d.utils import logger

# -------------------------------------------------
# Pillow – генерация простого FPS‑изображения
# -------------------------------------------------
from PIL import Image, ImageDraw, ImageFont


def make_fps_texture(fps: float, size: int = 64) -> bytes:
    """
    Возвращает RGBA‑байты (size×size) с надписью {fps:.1f}.
    Работает как с Pillow <10, так и с Pillow 10+.
    """
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Попытка использовать системный шрифт; иначе – дефолтный.
    try:
        font = ImageFont.truetype("arial.ttf", size // 2)
    except Exception:
        font = ImageFont.load_default()

    txt = f"{fps:.1f}"
    # Pillow 10+ убрал ImageDraw.textsize – используем font.getsize
    try:
        w, h = font.getsize(txt)
    except AttributeError:  # fallback для VERY старых Pillow
        bbox = draw.textbbox((0, 0), txt, font=font)
        w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]

    draw.text(((size - w) / 2, (size - h) / 2), txt,
              font=font, fill=(255, 255, 255, 255))
    return img.tobytes()


# -------------------------------------------------
# Генератор куба (функция из вашего оригинального примера)
# -------------------------------------------------
def make_cube() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    # Позиции
    v = np.array(
        [
            # Front (+Z)
            [-0.5, -0.5, +0.5],
            [+0.5, -0.5, +0.5],
            [+0.5, +0.5, +0.5],
            [-0.5, +0.5, +0.5],
            # Back (‑Z)
            [+0.5, -0.5, -0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, +0.5, -0.5],
            [+0.5, +0.5, -0.5],
            # Left (‑X)
            [-0.5, -0.5, -0.5],
            [-0.5, -0.5, +0.5],
            [-0.5, +0.5, +0.5],
            [-0.5, +0.5, -0.5],
            # Right (+X)
            [+0.5, -0.5, +0.5],
            [+0.5, -0.5, -0.5],
            [+0.5, +0.5, -0.5],
            [+0.5, +0.5, +0.5],
            # Top (+Y)
            [-0.5, +0.5, +0.5],
            [+0.5, +0.5, +0.5],
            [+0.5, +0.5, -0.5],
            [-0.5, +0.5, -0.5],
            # Bottom (‑Y)
            [-0.5, -0.5, -0.5],
            [+0.5, -0.5, -0.5],
            [+0.5, -0.5, +0.5],
            [-0.5, -0.5, +0.5],
        ],
        dtype=np.float32,
    )
    n = np.array(
        [
            # Front
            [0, 0, +1],
            [0, 0, +1],
            [0, 0, +1],
            [0, 0, +1],
            # Back
            [0, 0, -1],
            [0, 0, -1],
            [0, 0, -1],
            [0, 0, -1],
            # Left
            [-1, 0, 0],
            [-1, 0, 0],
            [-1, 0, 0],
            [-1, 0, 0],
            # Right
            [+1, 0, 0],
            [+1, 0, 0],
            [+1, 0, 0],
            [+1, 0, 0],
            # Top
            [0, +1, 0],
            [0, +1, 0],
            [0, +1, 0],
            [0, +1, 0],
            # Bottom
            [0, -1, 0],
            [0, -1, 0],
            [0, -1, 0],
            [0, -1, 0],
        ],
        dtype=np.float32,
    )
    uv = np.array([[0, 0], [1, 0], [1, 1], [0, 1]] * 6, dtype=np.float32)

    inds = []
    for i in range(0, 24, 4):
        inds.extend([i, i + 1, i + 2, i, i + 2, i + 3])
    inds = np.array(inds, dtype=np.uint32)

    return v, n, uv, inds


# -------------------------------------------------
# Сборка сцены (камеры, свет, куб, пол)
# -------------------------------------------------
def build_scene() -> Scene:
    scene = Scene()

    # Камера
    cam = Camera(fov=70.0, near=0.1, far=200.0)
    cam.position = Vec3(0.0, 1.5, 4.0)
    cam.rotation = Vec3(-15.0, 0.0, 0.0)
    scene.add_child(cam)
    scene.camera = cam

    # Свет
    sun = DirectionalLight(
        direction=Vec3(-0.6, -1.0, -0.4),
        color=Vec3(1.0, 0.95, 0.9),
        intensity=3.0,
    )
    scene.add_child(sun)

    lamp = PointLight(
        position=Vec3(2.0, 2.5, 0.0),
        radius=8.0,
        color=Vec3(1.0, 0.8, 0.6),
        intensity=2.5,
    )
    scene.add_child(lamp)

    # Куб
    verts, norms, uvs, inds = make_cube()
    cube = Mesh(
        vertices=verts,
        normals=norms,
        texcoords=uvs,
        indices=inds,
        name="RotatingCube",
    )
    cube.material = PBRMaterial(
        albedo=(0.85, 0.2, 0.15, 1.0),
        metallic=0.0,
        roughness=0.4,
        ao=1.0,
        emissive=(0.0, 0.0, 0.0),
    )
    cube.position = Vec3(0.0, 0.5, 0.0)
    scene.add_child(cube)

    # Пол
    floor_verts = np.array(
        [
            [-5.0, 0.0, -5.0],
            [+5.0, 0.0, -5.0],
            [+5.0, 0.0, +5.0],
            [-5.0, 0.0, +5.0],
        ],
        dtype=np.float32,
    )
    floor_norms = np.array([[0, 1, 0]] * 4, dtype=np.float32)
    floor_uvs = np.array([[0, 0], [5, 0], [5, 5], [0, 5]], dtype=np.float32)
    floor_inds = np.array([0, 1, 2, 0, 2, 3], dtype=np.uint32)

    floor = Mesh(
        vertices=floor_verts,
        normals=floor_norms,
        texcoords=floor_uvs,
        indices=floor_inds,
        name="Floor",
    )
    floor.material = PBRMaterial(
        albedo=(0.7, 0.7, 0.7, 1.0),
        metallic=0.0,
        roughness=0.9,
        ao=1.0,
    )
    scene.add_child(floor)

    return scene


# -------------------------------------------------
# Вращаем куб каждый кадр
# -------------------------------------------------
def attach_update_logic(scene: Scene):
    for node in scene.traverse():
        if node.name == "RotatingCube":
            def rotate_this(dt: float, node=node):
                speed = 30.0  # градусы/секунда
                node.rotation.y = (node.rotation.y + speed * dt) % 360.0
            node.on_update = rotate_this
            break


# -------------------------------------------------
# Точка входа
# -------------------------------------------------
def main() -> None:
    # Оставляем INFO‑уровень – в консоль будет только FPS раз в секунду
    logger.setLevel("INFO")

    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – Rotating Cube Demo",
        renderer="forward",
        backend_name="dx12",
    )

    scene = build_scene()
    attach_update_logic(scene)
    engine.scene = scene

    logger.info("[Demo] Starting main loop")
    engine.run()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        logger.exception("[Demo] Fatal error")
        raise
