# alkash3d/mesh/mesh.py
# -*- coding: utf-8 -*-
"""
Mesh – геометрический объект.
ИСПРАВЛЕННАЯ ВЕРСИЯ с подробной отладкой и исправленными импортами
"""

from __future__ import annotations
import numpy as np
import ctypes
from alkash3d.scene.node import Node
from alkash3d.utils import logger


class Mesh(Node):
    """
    Представляет геометрию в сцене.
    Содержит вершины, индексы и может быть отрисован рендерером.
    """

    def __init__(self,
                 vertices: np.ndarray,
                 indices: np.ndarray | None = None,
                 normals: np.ndarray | None = None,
                 texcoords: np.ndarray | None = None,
                 name: str = "Mesh"):
        super().__init__(name)

        # Приводим в нужный формат
        self.vertices = np.asarray(vertices, dtype=np.float32)
        if self.vertices.ndim == 1:
            self.vertices = self.vertices.reshape(-1, 3)

        self.indices = None
        if indices is not None:
            self.indices = np.asarray(indices, dtype=np.uint32)

        self.normals = None
        if normals is not None:
            self.normals = np.asarray(normals, dtype=np.float32)

        self.texcoords = None
        if texcoords is not None:
            self.texcoords = np.asarray(texcoords, dtype=np.float32)

        # Ссылки на GPU‑буферы
        self._vb = None
        self._ib = None

        # Видимость и материал
        self.visible = True
        self.material = None

        # Вычисляем bounding sphere
        self._compute_bounding_sphere()

        logger.info(f"[Mesh] Created {name}:")
        logger.info(f"  - Vertices: {len(self.vertices)}")
        logger.info(f"  - Indices: {len(self.indices) if self.indices is not None else 0}")
        if len(self.vertices) > 0:
            logger.info(f"  - First vertex: {self.vertices[0]}")
        if self.indices is not None and len(self.indices) > 0:
            logger.info(f"  - First indices: {self.indices[:6]}")

    def _compute_bounding_sphere(self):
        """Вычисляет сферу, охватывающую все вершины."""
        if len(self.vertices) == 0:
            self._bounding_center = np.array([0, 0, 0], dtype=np.float32)
            self._bounding_radius = 1.0
            return

        self._bounding_center = np.mean(self.vertices, axis=0)
        distances = np.linalg.norm(self.vertices - self._bounding_center, axis=1)
        self._bounding_radius = float(np.max(distances))

    @property
    def bounding_sphere(self):
        return self._bounding_center, self._bounding_radius

    @property
    def vertex_count(self) -> int:
        return len(self.vertices)

    # -----------------------------------------------------------------
    def draw(self, backend):
        """Отрисовка меша."""
        if not self.visible:
            logger.debug(f"[Mesh] {self.name} not visible, skipping")
            return

        logger.info(f"[Mesh] === Drawing {self.name} ===")

        # 1️⃣ Создаём vertex buffer
        if self._vb is None:
            logger.info(f"[Mesh] Creating vertex buffer for {self.name}")
            vertex_data = self.vertices.tobytes()
            logger.info(f"[Mesh] Vertex data size: {len(vertex_data)} bytes")
            logger.info(f"[Mesh] Vertex layout: {self.vertices.shape}, dtype={self.vertices.dtype}")

            self._vb = backend.create_buffer(vertex_data, usage="vertex")

            if self._vb and hasattr(self._vb, 'value'):
                logger.info(f"[Mesh] Vertex buffer created: {hex(self._vb.value)}")
            else:
                logger.error(f"[Mesh] FAILED to create vertex buffer for {self.name}")
                return
        else:
            logger.info(
                f"[Mesh] Vertex buffer already exists: {hex(self._vb.value if hasattr(self._vb, 'value') else 0)}")

        # 2️⃣ Создаём index buffer (если нужен) - отдельный буфер!
        if self.indices is not None and self._ib is None:
            logger.info(f"[Mesh] Creating index buffer for {self.name}")
            index_data = self.indices.tobytes()
            logger.info(f"[Mesh] Index data size: {len(index_data)} bytes")
            logger.info(f"[Mesh] First indices: {self.indices[:6]}")

            self._ib = backend.create_buffer(index_data, usage="index")

            if self._ib and hasattr(self._ib, 'value'):
                logger.info(f"[Mesh] Index buffer created: {hex(self._ib.value)}")
            else:
                logger.error(f"[Mesh] FAILED to create index buffer for {self.name}")

        # 3️⃣ Привязываем буферы
        if self._vb:
            # Проверяем, что вершинный и индексный буферы разные
            vb_val = self._vb.value if hasattr(self._vb, 'value') else int(self._vb)
            ib_val = 0
            if self._ib:
                ib_val = self._ib.value if hasattr(self._ib, 'value') else int(self._ib)
                if vb_val == ib_val:
                    logger.error(f"[Mesh] Vertex and index buffers have same address: 0x{vb_val:X}")
                    return

            logger.info(f"[Mesh] Setting vertex buffers")
            backend.set_vertex_buffers(self._vb, self._ib if self.indices is not None else None)

            # 4️⃣ Выполняем draw call
            if self.indices is not None and self._ib:
                logger.info(f"[Mesh] Drawing INDEXED: {len(self.indices)} indices")
                result = backend.draw_indexed(
                    len(self.indices),
                    start_index=0,
                    base_vertex=0,
                    instance_count=1
                )
                logger.info(f"[Mesh] Indexed draw result: {result}")
            else:
                logger.info(f"[Mesh] Drawing NON-INDEXED: {len(self.vertices)} vertices")
                result = backend.draw(
                    len(self.vertices),
                    start_vertex=0,
                    instance_count=1
                )
                logger.info(f"[Mesh] Non-indexed draw result: {result}")
        else:
            logger.error(f"[Mesh] No vertex buffer for {self.name}")

    def get_world_matrix(self):
        return super().get_world_matrix()