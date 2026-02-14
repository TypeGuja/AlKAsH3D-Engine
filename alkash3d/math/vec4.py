# alkash3d/math/vec4.py
"""
4‑мерный вектор (float32). Полезен, например, для RGBA‑цветов.
"""

import numpy as np
from typing import Iterable, Tuple


class Vec4:
    """Короткий и быстрый вектор‑4 (float32)."""

    __slots__ = ("_v",)

    def __init__(self, x: float = 0.0, y: float = 0.0,
                 z: float = 0.0, w: float = 0.0):
        self._v = np.array([x, y, z, w], dtype=np.float32)

    # -----------------------------------------------------------------
    # свойства (c‑сеттерами)
    # -----------------------------------------------------------------
    @property
    def x(self) -> float:
        return float(self._v[0])

    @x.setter
    def x(self, value: float) -> None:
        self._v[0] = float(value)

    @property
    def y(self) -> float:
        return float(self._v[1])

    @y.setter
    def y(self, value: float) -> None:
        self._v[1] = float(value)

    @property
    def z(self) -> float:
        return float(self._v[2])

    @z.setter
    def z(self, value: float) -> None:
        self._v[2] = float(value)

    @property
    def w(self) -> float:
        return float(self._v[3])

    @w.setter
    def w(self, value: float) -> None:
        self._v[3] = float(value)

    # -----------------------------------------------------------------
    # арифметика (операторы возвращают новый объект)
    # -----------------------------------------------------------------
    def __add__(self, other: "Vec4") -> "Vec4":
        return Vec4(*(self._v + other._v))

    def __sub__(self, other: "Vec4") -> "Vec4":
        return Vec4(*(self._v - other._v))

    def __mul__(self, scalar: float) -> "Vec4":
        return Vec4(*(self._v * scalar))

    __rmul__ = __mul__

    def __truediv__(self, scalar: float) -> "Vec4":
        return Vec4(*(self._v / scalar))

    # -----------------------------------------------------------------
    # вспомогательные методы
    # -----------------------------------------------------------------
    def dot(self, other: "Vec4") -> float:
        """Скалярное произведение."""
        return float(np.dot(self._v, other._v))

    def length(self) -> float:
        """Евклидова длина."""
        return float(np.linalg.norm(self._v))

    def normalized(self) -> "Vec4":
        """Нормализованный вектор."""
        n = self.length()
        if n == 0.0:
            return Vec4()
        return Vec4(*(self._v / n))

    def as_np(self) -> np.ndarray:
        """Копия 4‑компонентного ndarray (float32)."""
        return self._v.copy()

    # -----------------------------------------------------------------
    # представление
    # -----------------------------------------------------------------
    def __repr__(self) -> str:
        return f"Vec4({self.x:.3f}, {self.y:.3f}, {self.z:.3f}, {self.w:.3f})"

    # -----------------------------------------------------------------
    # приведение к кортежу/списку (удобно для передачи в OpenGL)
    # -----------------------------------------------------------------
    def to_tuple(self) -> Tuple[float, float, float, float]:
        return tuple(self._v.tolist())
