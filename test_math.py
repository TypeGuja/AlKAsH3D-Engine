# -*- coding: utf-8 -*-
import numpy as np
from alkash3d.math.vec3 import Vec3
from alkash3d.math.mat4 import Mat4
from alkash3d.math.quat import Quat

def test_vec3_ops():
    a = Vec3(1, 2, 3)
    b = Vec3(4, -1, 0)
    assert (a + b).as_np().tolist() == [5, 1, 3]
    assert (a - b).as_np().tolist() == [-3, 3, 3]
    assert (a * 2).as_np().tolist() == [2, 4, 6]

def test_mat4_identity():
    I = Mat4.identity()
    assert np.allclose(I.to_np(), np.eye(4, dtype=np.float32))

def test_mat4_translation():
    M = Mat4.translate(1, 2, 3)
    p = np.array([0, 0, 0, 1], dtype=np.float32)
    res = M.to_np() @ p
    assert np.allclose(res, np.array([1, 2, 3, 1], dtype=np.float32))

def test_quat_rotation():
    q = Quat.from_axis_angle(Vec3(0, 1, 0), np.radians(90))
    v = Vec3(1, 0, 0)
    rotated = q * v * q.conjugate()
    assert np.allclose(rotated.as_np(), np.array([0, 0, -1], dtype=np.float32), rotated)
