"""
Трёхмерный вектор – базовый тип на основе NumPy.
"""

import numpy as np


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

    # Добавьте эти property для совместимости
    @property
    def data(self) -> np.ndarray:
        return self._data

    def __add__(self, other: 'Vec3') -> 'Vec3':
        if hasattr(other, '_data'):
            return Vec3(*(self._data + other._data))
        return Vec3(*(self._data + other))

    def __sub__(self, other: 'Vec3') -> 'Vec3':
        if hasattr(other, '_data'):
            return Vec3(*(self._data - other._data))
        return Vec3(*(self._data - other))

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
        if hasattr(other, '_data'):
            return float(np.dot(self._data, other._data))
        return float(np.dot(self._data, other))

    def cross(self, other: 'Vec3') -> 'Vec3':
        if hasattr(other, '_data'):
            return Vec3(*np.cross(self._data, other._data))
        return Vec3(*np.cross(self._data, other))

    def to_bytes(self) -> bytes:
        return self._data.tobytes()

    def as_np(self) -> np.ndarray:
        """Для совместимости с существующим кодом"""
        return self._data