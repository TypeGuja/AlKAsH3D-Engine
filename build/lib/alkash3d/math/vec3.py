# -*- coding: utf-8 -*-
"""
Трёхмерный вектор на базе NumPy (для простоты – без надстройки над __array_interface__).
"""
import numpy as np

class Vec3:
    __slots__ = ("_v",)

    def __init__(self, x=0.0, y=0.0, z=0.0):
        self._v = np.array([x, y, z], dtype=np.float32)

    # -------------------------------------------------
    # свойства с сеттерами
    # -------------------------------------------------
    @property
    def x(self) -> float:
        return float(self._v[0])

    @x.setter
    def x(self, value: float):
        self._v[0] = float(value)

    @property
    def y(self) -> float:
        return float(self._v[1])

    @y.setter
    def y(self, value: float):
        self._v[1] = float(value)

    @property
    def z(self) -> float:
        return float(self._v[2])

    @z.setter
    def z(self, value: float):
        self._v[2] = float(value)

    # -------------------------------------------------
    # арифметика (не меняет исходный объект)
    # -------------------------------------------------
    def __add__(self, other):
        return Vec3(*(self._v + other._v))

    def __sub__(self, other):
        return Vec3(*(self._v - other._v))

    def __mul__(self, scalar):
        return Vec3(*(self._v * scalar))

    __rmul__ = __mul__

    # -------------------------------------------------
    # вспомогательные методы
    # -------------------------------------------------
    def dot(self, other):
        return float(np.dot(self._v, other._v))

    def cross(self, other):
        return Vec3(*np.cross(self._v, other._v))

    def length(self):
        return float(np.linalg.norm(self._v))

    def normalized(self):
        n = self.length()
        if n == 0.0:
            return Vec3()
        return Vec3(*(self._v / n))

    def as_np(self) -> np.ndarray:
        """Возврат копии 3‑элементного массива float32."""
        return self._v.copy()

    def __repr__(self):
        return f"Vec3({self.x:.3f}, {self.y:.3f}, {self.z:.3f})"