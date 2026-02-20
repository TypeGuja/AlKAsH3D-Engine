# alkash3d/math/matrix.py
"""
Матричная математика для 3D графики
"""

import numpy as np
from typing import Optional
from .vector import Vec3, Vec4


class Mat4:
    """Матрица 4x4"""

    def __init__(self, data: Optional[np.ndarray] = None):
        if data is not None:
            self._data = data.astype(np.float32)
        else:
            self._data = np.eye(4, dtype=np.float32)

    @classmethod
    def identity(cls) -> 'Mat4':
        """Создать единичную матрицу"""
        return cls(np.eye(4, dtype=np.float32))

    @classmethod
    def translation(cls, v: Vec3) -> 'Mat4':
        """Матрица переноса"""
        m = np.eye(4, dtype=np.float32)
        m[0, 3] = v.x
        m[1, 3] = v.y
        m[2, 3] = v.z
        return cls(m)

    @classmethod
    def rotation_x(cls, angle: float) -> 'Mat4':
        """Матрица поворота вокруг оси X"""
        c = np.cos(angle)
        s = np.sin(angle)
        m = np.eye(4, dtype=np.float32)
        m[1, 1] = c
        m[1, 2] = -s
        m[2, 1] = s
        m[2, 2] = c
        return cls(m)

    @classmethod
    def rotation_y(cls, angle: float) -> 'Mat4':
        """Матрица поворота вокруг оси Y"""
        c = np.cos(angle)
        s = np.sin(angle)
        m = np.eye(4, dtype=np.float32)
        m[0, 0] = c
        m[0, 2] = s
        m[2, 0] = -s
        m[2, 2] = c
        return cls(m)

    @classmethod
    def rotation_z(cls, angle: float) -> 'Mat4':
        """Матрица поворота вокруг оси Z"""
        c = np.cos(angle)
        s = np.sin(angle)
        m = np.eye(4, dtype=np.float32)
        m[0, 0] = c
        m[0, 1] = -s
        m[1, 0] = s
        m[1, 1] = c
        return cls(m)

    @classmethod
    def scale(cls, v: Vec3) -> 'Mat4':
        """Матрица масштабирования"""
        m = np.eye(4, dtype=np.float32)
        m[0, 0] = v.x
        m[1, 1] = v.y
        m[2, 2] = v.z
        return cls(m)

    @classmethod
    def perspective(cls, fov: float, aspect: float, near: float, far: float) -> 'Mat4':
        """Матрица перспективной проекции"""
        tan_half_fov = np.tan(fov / 2)
        m = np.zeros((4, 4), dtype=np.float32)
        m[0, 0] = 1 / (aspect * tan_half_fov)
        m[1, 1] = 1 / tan_half_fov
        m[2, 2] = far / (near - far)
        m[2, 3] = -1
        m[3, 2] = -(far * near) / (far - near)
        return cls(m)

    @classmethod
    def look_at(cls, eye: Vec3, target: Vec3, up: Vec3) -> 'Mat4':
        """Матрица вида камеры"""
        z = (eye - target).normalize()
        x = up.cross(z).normalize()
        y = z.cross(x)

        m = np.eye(4, dtype=np.float32)
        m[0, 0] = x.x
        m[0, 1] = x.y
        m[0, 2] = x.z
        m[1, 0] = y.x
        m[1, 1] = y.y
        m[1, 2] = y.z
        m[2, 0] = z.x
        m[2, 1] = z.y
        m[2, 2] = z.z
        m[0, 3] = -x.dot(eye)
        m[1, 3] = -y.dot(eye)
        m[2, 3] = -z.dot(eye)
        return cls(m)

    def __mul__(self, other: 'Mat4') -> 'Mat4':
        return Mat4(self._data @ other._data)

    def __getitem__(self, key):
        return self._data[key]

    def to_bytes(self) -> bytes:
        return self._data.tobytes()

    def transpose(self) -> 'Mat4':
        return Mat4(self._data.T)

    def inverse(self) -> 'Mat4':
        return Mat4(np.linalg.inv(self._data))