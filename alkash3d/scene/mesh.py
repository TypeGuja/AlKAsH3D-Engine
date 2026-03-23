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

        logger.debug(
            f"[Mesh] Created {name} with {self.vertex_count} vertices, indices: {len(self.indices) if self.indices is not None else 0}")

    # -----------------------------------------------------------------
    # Properties для bounding sphere
    # -----------------------------------------------------------------
    @property
    def bounding_sphere(self):
        """Возвращает (центр, радиус) для culling."""
        return self._bounding_center, self._bounding_radius

    # -----------------------------------------------------------------
    # Compatibility API
    # -----------------------------------------------------------------
    @property
    def vertex_count(self) -> int:
        """Return number of vertices (or number of indices if an index buffer is used)."""
        return self.index_count

    # -----------------------------------------------------------------
    # Draw method
    # -----------------------------------------------------------------
    def draw(self, backend):
        """
        Создаёт (при первом вызове) vertex‑/index‑буферы,
        привязывает их к командному списку и отрисовывает.
        """
        logger.debug(f"[Mesh] Drawing {self.name}, vertices: {self.vertex_count}")

        # 1️⃣ Сначала создаём буферы, если их ещё нет
        if self._vb is None:
            # vertices → raw bytes, layout 3×float32 = 12 байт
            logger.debug(f"[Mesh] Creating vertex buffer for {self.name}")
            vertex_data = self.vertices.tobytes()
            logger.debug(f"[Mesh] Vertex data size: {len(vertex_data)} bytes")
            self._vb = backend.create_buffer(vertex_data, usage="vertex")
            if self._vb:
                logger.debug(
                    f"[Mesh] Vertex buffer created: {hex(self._vb.value if hasattr(self._vb, 'value') else int(self._vb))}")

        if self.indices is not None and self._ib is None:
            logger.debug(f"[Mesh] Creating index buffer for {self.name}")
            index_data = self.indices.tobytes()
            logger.debug(f"[Mesh] Index data size: {len(index_data)} bytes")
            self._ib = backend.create_buffer(index_data, usage="index")
            if self._ib:
                logger.debug(
                    f"[Mesh] Index buffer created: {hex(self._ib.value if hasattr(self._ib, 'value') else int(self._ib))}")

        # 2️⃣ Привязываем буферы к пайплайну
        if self._vb:
            logger.debug(f"[Mesh] Setting vertex buffers for {self.name}")
            backend.set_vertex_buffers(self._vb, self._ib)
        else:
            logger.error(f"[Mesh] No vertex buffer for {self.name}")
            return

        # 3️⃣ Выполняем draw‑call
        if self.indices is not None:
            # index‑based draw
            logger.debug(f"[Mesh] Drawing indexed: {len(self.indices)} indices")
            backend.draw_indexed(
                index_count=len(self.indices),
                start_index=0,
                base_vertex=0,
                instance_count=1
            )
        else:
            # простой non‑indexed draw
            vertex_count = len(self.vertices) if self.vertices.ndim == 1 else self.vertices.shape[0]
            logger.debug(f"[Mesh] Drawing non-indexed: {vertex_count} vertices")
            backend.draw(
                vertex_count=vertex_count,
                start_vertex=0,
                instance_count=1
            )