# alkash3d/math/vector.py
"""
Векторная математика для 3D графики
"""

import numpy as np
from typing import Union, Tuple


class Vec3:
    """3D вектор"""

    def __init__(self, x: float = 0.0, y: float = 0.0, z: float = 0.0):
        self._data = np.array([x, y, z], dtype=np.float32)

    @property
    def x(self) -> float:
        return self._data[0]

    @x.setter
    def x(self, value: float):
        self._data[0] = value

    @property
    def y(self) -> float:
        return self._data[1]

    @y.setter
    def y(self, value: float):
        self._data[1] = value

    @property
    def z(self) -> float:
        return self._data[2]

    @z.setter
    def z(self, value: float):
        self._data[2] = value

    def __add__(self, other: 'Vec3') -> 'Vec3':
        return Vec3(*(self._data + other._data))

    def __sub__(self, other: 'Vec3') -> 'Vec3':
        return Vec3(*(self._data - other._data))

    def __mul__(self, scalar: float) -> 'Vec3':
        return Vec3(*(self._data * scalar))

    def __repr__(self) -> str:
        return f"Vec3({self.x:.2f}, {self.y:.2f}, {self.z:.2f})"

    def length(self) -> float:
        return float(np.linalg.norm(self._data))

    def normalize(self) -> 'Vec3':
        length = self.length()
        if length > 0:
            return Vec3(*(self._data / length))
        return Vec3()

    def dot(self, other: 'Vec3') -> float:
        return float(np.dot(self._data, other._data))

    def cross(self, other: 'Vec3') -> 'Vec3':
        return Vec3(*np.cross(self._data, other._data))

    def to_bytes(self) -> bytes:
        return self._data.tobytes()

    # ------------------------------------------------------------------
    # Compatibility API
    # ------------------------------------------------------------------
    def as_np(self) -> np.ndarray:
        """Return a copy of the internal NumPy array (float32)."""
        return self._data.copy()


class Vec4:
    """4D вектор"""

    def __init__(self, x: float = 0.0, y: float = 0.0,
                 z: float = 0.0, w: float = 1.0):
        self._data = np.array([x, y, z, w], dtype=np.float32)

    @property
    def x(self) -> float:
        return self._data[0]

    @x.setter
    def x(self, value: float):
        self._data[0] = value

    @property
    def y(self) -> float:
        return self._data[1]

    @y.setter
    def y(self, value: float):
        self._data[1] = value

    @property
    def z(self) -> float:
        return self._data[2]

    @z.setter
    def z(self, value: float):
        self._data[2] = value

    @property
    def w(self) -> float:
        return self._data[3]

    @w.setter
    def w(self, value: float):
        self._data[3] = value

    def __repr__(self) -> str:
        return f"Vec4({self.x:.2f}, {self.y:.2f}, {self.z:.2f}, {self.w:.2f})"

    def to_bytes(self) -> bytes:
        return self._data.tobytes()