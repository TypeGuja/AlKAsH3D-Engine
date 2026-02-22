# alkash3d/math/transform.py
"""
Набор вспомогательных функций построения матриц преобразования.
Все функции возвращают объект ``Mat4`` из пакета ``alkash3d.math.mat4``.
"""

from __future__ import annotations
import numpy as np
from .vec3 import Vec3
from .mat4 import Mat4


def translation(v: Vec3) -> Mat4:
    """Матрица переноса на вектор ``v``."""
    return Mat4.translate(v.x, v.y, v.z)


def rotation_x(angle_deg: float) -> Mat4:
    """Матрица поворота вокруг оси X."""
    return Mat4.rotate_x(angle_deg)


def rotation_y(angle_deg: float) -> Mat4:
    """Матрица поворота вокруг оси Y."""
    return Mat4.rotate_y(angle_deg)


def rotation_z(angle_deg: float) -> Mat4:
    """Матрица поворота вокруг оси Z."""
    return Mat4.rotate_z(angle_deg)


def scaling(v: Vec3) -> Mat4:
    """Матрица масштабирования."""
    return Mat4.scale(v.x, v.y, v.z)


def look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4:
    """Матрица вида, смотрящая из ``eye`` в ``target`` с вектором ``up``."""
    return Mat4.look_at(eye, target, up)
