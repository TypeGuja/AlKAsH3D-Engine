# alkash3d/math/quaternion.py
"""
Кватернионы для 3D вращений
"""

import numpy as np
from .vector import Vec3
from .matrix import Mat4


class Quat:
    """Кватернион для представления вращений"""

    def __init__(self, w: float = 1.0, x: float = 0.0, y: float = 0.0, z: float = 0.0):
        self._data = np.array([w, x, y, z], dtype=np.float32)
        self.normalize()

    @property
    def w(self) -> float:
        return self._data[0]

    @property
    def x(self) -> float:
        return self._data[1]

    @property
    def y(self) -> float:
        return self._data[2]

    @property
    def z(self) -> float:
        return self._data[3]

    @classmethod
    def identity(cls) -> 'Quat':
        """Единичный кватернион"""
        return cls(1.0, 0.0, 0.0, 0.0)

    @classmethod
    def from_axis_angle(cls, axis: Vec3, angle: float) -> 'Quat':
        """Создать из оси и угла"""
        axis = axis.normalize()
        half_angle = angle / 2
        sin_half = np.sin(half_angle)
        return cls(
            np.cos(half_angle),
            axis.x * sin_half,
            axis.y * sin_half,
            axis.z * sin_half
        )

    @classmethod
    def from_euler(cls, pitch: float, yaw: float, roll: float) -> 'Quat':
        """Создать из углов Эйлера"""
        cy = np.cos(yaw * 0.5)
        sy = np.sin(yaw * 0.5)
        cp = np.cos(pitch * 0.5)
        sp = np.sin(pitch * 0.5)
        cr = np.cos(roll * 0.5)
        sr = np.sin(roll * 0.5)

        w = cr * cp * cy + sr * sp * sy
        x = sr * cp * cy - cr * sp * sy
        y = cr * sp * cy + sr * cp * sy
        z = cr * cp * sy - sr * sp * cy

        return cls(w, x, y, z)

    def normalize(self):
        """Нормализовать кватернион"""
        norm = np.linalg.norm(self._data)
        if norm > 0:
            self._data /= norm

    def __mul__(self, other: 'Quat') -> 'Quat':
        """Умножение кватернионов (композиция вращений)"""
        w = self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z
        x = self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y
        y = self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x
        z = self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w
        return Quat(w, x, y, z)

    def rotate(self, v: Vec3) -> Vec3:
        """Повернуть вектор кватернионом"""
        q_vec = Vec3(self.x, self.y, self.z)
        uv = q_vec.cross(v)
        uuv = q_vec.cross(uv)
        return v + (uv * (2 * self.w)) + (uuv * 2)

    def to_matrix(self) -> Mat4:
        """Преобразовать в матрицу поворота"""
        xx = self.x * self.x
        xy = self.x * self.y
        xz = self.x * self.z
        xw = self.x * self.w
        yy = self.y * self.y
        yz = self.y * self.z
        yw = self.y * self.w
        zz = self.z * self.z
        zw = self.z * self.w

        m = np.eye(4, dtype=np.float32)
        m[0, 0] = 1 - 2 * (yy + zz)
        m[0, 1] = 2 * (xy - zw)
        m[0, 2] = 2 * (xz + yw)
        m[1, 0] = 2 * (xy + zw)
        m[1, 1] = 1 - 2 * (xx + zz)
        m[1, 2] = 2 * (yz - xw)
        m[2, 0] = 2 * (xz - yw)
        m[2, 1] = 2 * (yz + xw)
        m[2, 2] = 1 - 2 * (xx + yy)

        return Mat4(m)