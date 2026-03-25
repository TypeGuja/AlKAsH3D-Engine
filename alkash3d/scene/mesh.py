# alkash3d/scene/mesh.py
"""
Mesh – геометрический объект сцены.
"""

import numpy as np
from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3
from typing import Optional
from alkash3d.utils import logger


class Mesh(Node):
    """Примитивный объект – лениво создаёт буферы при первом draw()."""

    def __init__(self,
                 vertices: np.ndarray,
                 indices: Optional[np.ndarray] = None,
                 normals: Optional[np.ndarray] = None,
                 texcoords: Optional[np.ndarray] = None,
                 name: str = "Mesh"):
        super().__init__(name)

        self.vertices = vertices.astype(np.float32)
        self.normals = normals.astype(np.float32) if normals is not None else None
        self.texcoords = texcoords.astype(np.float32) if texcoords is not None else None
        self.indices = indices.astype(np.uint32) if indices is not None else None

        # количество индексов/вершин
        if self.indices is not None:
            self.index_count = len(self.indices)
        else:
            if self.vertices.ndim == 1:
                self.vertices = self.vertices.reshape(-1, 3)
            self.index_count = len(self.vertices)

        # базовый цвет
        self.color = Vec3(1.0, 1.0, 1.0)

        # GPU-буферы (будут созданы при первом draw)
        self._vb: Optional[any] = None
        self._ib: Optional[any] = None

        # Видимость
        self.visible = True

        # Bounding‑sphere (для culling)
        verts = self.vertices
        if verts.ndim == 1:
            verts = verts.reshape((-1, 3))
        self._bounding_center = verts.mean(axis=0).astype(np.float32)
        self._bounding_radius = float(np.linalg.norm(verts - self._bounding_center, axis=1).max())

        logger.info(
            f"[Mesh] Created {name} with {self.vertex_count} vertices, indices: {len(self.indices) if self.indices is not None else 0}")

    # -----------------------------------------------------------------
    @property
    def vertex_count(self) -> int:
        """Return number of vertices (or number of indices if an index buffer is used)."""
        return self.index_count

    @property
    def bounding_sphere(self):
        """Возвращает (центр, радиус) для culling."""
        return self._bounding_center, self._bounding_radius

    # -----------------------------------------------------------------
    def draw(self, backend):
        """
        Создаёт (при первом вызове) vertex‑/index‑буферы,
        привязывает их к командному списку и отрисовывает.
        """
        logger.debug(f"[Mesh] Drawing {self.name}, vertices: {self.vertex_count}")

        # 1️⃣ Создаём буферы, если их ещё нет
        if self._vb is None:
            vertex_data = self.vertices.tobytes()
            logger.debug(f"[Mesh] Creating vertex buffer, size: {len(vertex_data)} bytes")
            self._vb = backend.create_buffer(vertex_data, usage="vertex")
            if not self._vb or not self._vb.value:
                raise RuntimeError(f"Failed to create vertex buffer for {self.name}")

        if self.indices is not None and self._ib is None:
            index_data = self.indices.tobytes()
            logger.debug(f"[Mesh] Creating index buffer, size: {len(index_data)} bytes")
            self._ib = backend.create_buffer(index_data, usage="index")
            if not self._ib or not self._ib.value:
                raise RuntimeError(f"Failed to create index buffer for {self.name}")

        # 2️⃣ Привязываем буферы
        backend.set_vertex_buffers(self._vb, self._ib)

        # 3️⃣ Выполняем draw‑call
        if self.indices is not None:
            logger.debug(f"[Mesh] Drawing indexed: {len(self.indices)} indices")
            backend.draw_indexed(
                index_count=len(self.indices),
                start_index=0,
                base_vertex=0,
                instance_count=1
            )
        else:
            vertex_count = len(self.vertices) if self.vertices.ndim == 1 else self.vertices.shape[0]
            logger.debug(f"[Mesh] Drawing non-indexed: {vertex_count} vertices")
            backend.draw(
                vertex_count=vertex_count,
                start_vertex=0,
                instance_count=1
            )