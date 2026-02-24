# alkash3d/scene/mesh.py
"""
Mesh – геометрический объект сцены.
"""

import numpy as np
from alkash3d.scene.node import Node  # ИМПОРТИРУЕМ NODE
from alkash3d.math.vec3 import Vec3
from typing import Optional


class Mesh(Node):  # НАСЛЕДУЕМ ОТ NODE
    """Примитивный объект – лениво создаёт буферы при первом draw()."""

    def __init__(self,
                 vertices: np.ndarray,
                 indices: Optional[np.ndarray] = None,
                 normals: Optional[np.ndarray] = None,
                 texcoords: Optional[np.ndarray] = None,
                 name: str = "Mesh"):
        super().__init__(name)  # ВЫЗЫВАЕМ КОНСТРУКТОР NODE

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
        # 1️⃣ Сначала создаём буферы, если их ещё нет
        if self._vb is None:
            # vertices → raw bytes, layout 3×float32 = 12 байт
            self._vb = backend.create_buffer(self.vertices.tobytes(), usage="vertex")

        if self.indices is not None and self._ib is None:
            self._ib = backend.create_buffer(self.indices.tobytes(), usage="index")

        # 2️⃣ Привязываем буферы к пайплайну
        backend.set_vertex_buffers(self._vb, self._ib)

        # 3️⃣ Выполняем draw‑call
        if self.indices is not None:
            # index‑based draw
            backend.draw_indexed(
                index_count=len(self.indices),
                start_index=0,
                base_vertex=0,
                instance_count=1
            )
        else:
            # простой non‑indexed draw
            vertex_count = len(self.vertices) if self.vertices.ndim == 1 else self.vertices.shape[0]
            backend.draw(
                vertex_count=vertex_count,
                start_vertex=0,
                instance_count=1
            )