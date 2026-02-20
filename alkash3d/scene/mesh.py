# alkash3d/scene/mesh.py
"""
Mesh – геометрический объект сцены.
"""

import numpy as np
from alkash3d.math.vec3 import Vec3


class Mesh:
    """Примитивный объект – лениво создаёт VAO при первом draw()."""

    def __init__(self,
                 vertices: np.ndarray,
                 normals: np.ndarray = None,
                 texcoords: np.ndarray = None,
                 indices: np.ndarray = None,
                 name="Mesh"):
        self.vertices = vertices.astype(np.float32)
        self.normals = normals.astype(np.float32) if normals is not None else None
        self.texcoords = texcoords.astype(np.float32) if texcoords is not None else None
        self.indices = indices.astype(np.uint32) if indices is not None else None

        # количество индексов/вершин
        self.index_count = len(self.indices) if self.indices is not None else len(self.vertices) // 3

        # базовый цвет
        self.color = Vec3(1.0, 1.0, 1.0)

        # Bounding‑sphere (для culling)
        verts = self.vertices
        if verts.ndim == 1:
            verts = verts.reshape((-1, 3))
        self._bounding_center = verts.mean(axis=0).astype(np.float32)
        self._bounding_radius = np.linalg.norm(verts - self._bounding_center, axis=1).max()

    # -----------------------------------------------------------------
    # Compatibility API – тесты ожидают атрибут ``vertex_count``.
    # -----------------------------------------------------------------
    @property
    def vertex_count(self) -> int:
        """Return number of vertices (or number of indices if an index buffer is used)."""
        return self.index_count