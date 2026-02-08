# alkas3d/math/quat.py
# ---------------------------------------------------------------
# Краткая реализация кватернионов (x, y, z, w) с поддержкой:
# - создания из угла/оси,
# - умножения,
# - нормализации,
# - преобразования в 4×4 матрицу,
# - вращения вектора.
# ---------------------------------------------------------------

import numpy as np
from math import sin, cos, radians, sqrt

class Quat:
    __slots__ = ("x", "y", "z", "w")

    def __init__(self, x=0.0, y=0.0, z=0.0, w=1.0):
        self.x = float(x)
        self.y = float(y)
        self.z = float(z)
        self.w = float(w)

    @staticmethod
    def from_axis_angle(axis, angle_deg):
        """axis – 3‑элементный iterable, angle – в градусах."""
        a = radians(angle_deg) / 2.0
        s = sin(a)
        ax = np.array(axis, dtype=np.float32)
        ax = ax / np.linalg.norm(ax)
        return Quat(ax[0] * s, ax[1] * s, ax[2] * s, cos(a))

    @staticmethod
    def from_euler(pitch, yaw, roll):
        """Эйлеровы углы в градусах (X‑pitch, Y‑yaw, Z‑roll)."""
        qx = Quat.from_axis_angle([1, 0, 0], pitch)
        qy = Quat.from_axis_angle([0, 1, 0], yaw)
        qz = Quat.from_axis_angle([0, 0, 1], roll)
        # порядок: yaw → pitch → roll (как в Mat4.from_euler)
        return qy * qx * qz

    def __mul__(self, other: "Quat") -> "Quat":
        """Гамма‑умножение кватернионов."""
        x = self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y
        y = self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x
        z = self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w
        w = self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z
        return Quat(x, y, z, w)

    def normalized(self) -> "Quat":
        n = sqrt(self.x**2 + self.y**2 + self.z**2 + self.w**2)
        if n == 0:
            return Quat()
        inv = 1.0 / n
        return Quat(self.x*inv, self.y*inv, self.z*inv, self.w*inv)

    # -----------------------------------------------------------
    #  Преобразования
    # -----------------------------------------------------------
    def to_mat4(self):
        """Возвращает 4×4 матрицу вращения."""
        x, y, z, w = self.x, self.y, self.z, self.w
        xx, yy, zz = x*x, y*y, z*z
        xy, xz, yz = x*y, x*z, y*z
        wx, wy, wz = w*x, w*y, w*z

        m = np.identity(4, dtype=np.float32)
        m[0, 0] = 1 - 2*(yy + zz)
        m[0, 1] = 2*(xy - wz)
        m[0, 2] = 2*(xz + wy)

        m[1, 0] = 2*(xy + wz)
        m[1, 1] = 1 - 2*(xx + zz)
        m[1, 2] = 2*(yz - wx)

        m[2, 0] = 2*(xz - wy)
        m[2, 1] = 2*(yz + wx)
        m[2, 2] = 1 - 2*(xx + yy)

        return m

    def rotate_vector(self, vec):
        """Вращает 3‑D вектор `vec` (np.ndarray length‑3)."""
        qvec = Quat(vec[0], vec[1], vec[2], 0.0)
        res = self * qvec * self.conjugate()
        return np.array([res.x, res.y, res.z], dtype=np.float32)

    def conjugate(self):
        return Quat(-self.x, -self.y, -self.z, self.w)

    def __repr__(self):
        return f"Quat({self.x:.3f}, {self.y:.3f}, {self.z:.3f}, {self.w:.3f})"