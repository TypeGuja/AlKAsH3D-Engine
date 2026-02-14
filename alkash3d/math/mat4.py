# alkash3d/math/mat4.py
import numpy as np
from math import radians, tan, sin, cos


class Mat4:
    __slots__ = ("m",)

    def __init__(self, array: np.ndarray = None):
        if array is None:
            self.m = np.identity(4, dtype=np.float32)
        else:
            self.m = np.array(array, dtype=np.float32).reshape((4, 4))

    @staticmethod
    def identity():
        return Mat4(np.identity(4, dtype=np.float32))

    @staticmethod
    def translate(x: float, y: float, z: float):
        m = np.identity(4, dtype=np.float32)
        m[0, 3] = x
        m[1, 3] = y
        m[2, 3] = z
        return Mat4(m)

    @staticmethod
    def scale(sx: float, sy: float, sz: float):
        m = np.identity(4, dtype=np.float32)
        m[0, 0] = sx
        m[1, 1] = sy
        m[2, 2] = sz
        return Mat4(m)

    @staticmethod
    def rotate_x(angle_deg: float):
        a = radians(angle_deg)
        c, s = cos(a), sin(a)
        m = np.identity(4, dtype=np.float32)
        m[1, 1] = c
        m[1, 2] = -s
        m[2, 1] = s
        m[2, 2] = c
        return Mat4(m)

    @staticmethod
    def rotate_y(angle_deg: float):
        a = radians(angle_deg)
        c, s = cos(a), sin(a)
        m = np.identity(4, dtype=np.float32)
        m[0, 0] = c
        m[0, 2] = s
        m[2, 0] = -s
        m[2, 2] = c
        return Mat4(m)

    @staticmethod
    def rotate_z(angle_deg: float):
        a = radians(angle_deg)
        c, s = cos(a), sin(a)
        m = np.identity(4, dtype=np.float32)
        m[0, 0] = c
        m[0, 1] = -s
        m[1, 0] = s
        m[1, 1] = c
        return Mat4(m)

    @staticmethod
    def from_euler(pitch: float, yaw: float, roll: float):
        Rx = Mat4.rotate_x(pitch)
        Ry = Mat4.rotate_y(yaw)
        Rz = Mat4.rotate_z(roll)
        return Ry @ Rx @ Rz

    @staticmethod
    def perspective(fov_deg: float, aspect: float,
                    z_near: float, z_far: float):
        f = 1.0 / tan(radians(fov_deg) / 2.0)
        m = np.zeros((4, 4), dtype=np.float32)
        m[0, 0] = f / aspect
        m[1, 1] = f
        m[2, 2] = (z_far + z_near) / (z_near - z_far)
        m[2, 3] = (2.0 * z_far * z_near) / (z_near - z_far)
        m[3, 2] = -1.0
        return Mat4(m)

    @staticmethod
    def look_at(eye, target, up) -> "Mat4":
        f = (target - eye).astype(np.float32)
        f = f / np.linalg.norm(f)

        u = up.astype(np.float32)
        u = u / np.linalg.norm(u)

        s = np.cross(f, u)
        s = s / np.linalg.norm(s)

        u = np.cross(s, f)

        m = np.identity(4, dtype=np.float32)
        m[0, :3] = s
        m[1, :3] = u
        m[2, :3] = -f

        m[0, 3] = -np.dot(s, eye)
        m[1, 3] = -np.dot(u, eye)
        m[2, 3] = np.dot(f, eye)

        return Mat4(m)

    def __matmul__(self, other: "Mat4") -> "Mat4":
        return Mat4(np.dot(self.m, other.m))

    def __repr__(self):
        return f"Mat4({self.m})"

    def to_np(self) -> np.ndarray:
        return self.m.copy()

    def to_gl(self) -> np.ndarray:
        """Транспонируем для передачи в OpenGL (столбцы‑массив)."""
        return self.m.T.copy()