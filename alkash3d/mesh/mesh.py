# alkash3d/scene/mesh.py
# -*- coding: utf-8 -*-
"""
Mesh – геометрический объект сцены.
ИСПРАВЛЕННАЯ ВЕРСИЯ с подробной отладкой
"""

from __future__ import annotations
import numpy as np
import ctypes
from alkash3d.scene.node import Node
from alkash3d.utils.logger import logger


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

        # Ссылки на GPU‑буферы, которые создаются «лениво» при первом draw
        self._vb = None  # vertex buffer (ctypes.c_void_p)
        self._ib = None  # index buffer (ctypes.c_void_p) – может быть None

        # Видимость – можно отключать объектом
        self.visible = True

        # Вычисляем bounding sphere для culling
        self._compute_bounding_sphere()

        logger.info(
            f"[Mesh] Created {name} with {len(self.vertices)} vertices, {len(self.indices) if self.indices is not None else 0} indices")

    def _compute_bounding_sphere(self):
        """Вычисляет сферу, охватывающую все вершины."""
        if len(self.vertices) == 0:
            self._bounding_center = np.array([0, 0, 0], dtype=np.float32)
            self._bounding_radius = 1.0
            return

        # Центр = среднее всех вершин
        self._bounding_center = np.mean(self.vertices, axis=0)

        # Радиус = максимальное расстояние от центра
        distances = np.linalg.norm(self.vertices - self._bounding_center, axis=1)
        self._bounding_radius = float(np.max(distances))

        logger.debug(f"[Mesh] {self.name}: center={self._bounding_center}, radius={self._bounding_radius}")

    @property
    def bounding_sphere(self):
        """Возвращает (центр, радиус) в локальных координатах."""
        return self._bounding_center, self._bounding_radius

    # -----------------------------------------------------------------
    # Простейший draw‑метод, совместимый с DX12Backend
    # -----------------------------------------------------------------
    def draw(self, backend):
        """
        Создаёт (при первом вызове) vertex‑/index‑буферы,
        привязывает их к командному списку и отрисовывает.
        """
        if not self.visible:
            logger.debug(f"[Mesh] {self.name} not visible, skipping")
            return

        logger.info(f"[Mesh] Drawing {self.name}")

        # 1️⃣ Сначала создаём буферы, если их ещё нет
        if self._vb is None:
            logger.info(f"[Mesh] Creating vertex buffer for {self.name}, size={len(self.vertices)} vertices")
            # vertices → raw bytes, layout 3×float32 = 12 байт на вершину
            vertex_data = self.vertices.tobytes()
            logger.info(f"[Mesh] Vertex data size: {len(vertex_data)} bytes")
            logger.info(f"[Mesh] First few vertices: {self.vertices[:3]}")

            self._vb = backend.create_buffer(vertex_data, usage="vertex")
            if self._vb and hasattr(self._vb, 'value') and self._vb.value:
                logger.info(f"[Mesh] Vertex buffer created: {hex(self._vb.value)}")
            else:
                logger.error(f"[Mesh] Failed to create vertex buffer for {self.name}")
                return

        if self.indices is not None and self._ib is None:
            logger.info(f"[Mesh] Creating index buffer for {self.name}, size={len(self.indices)} indices")
            index_data = self.indices.tobytes()
            logger.info(f"[Mesh] Index data size: {len(index_data)} bytes")
            logger.info(f"[Mesh] First few indices: {self.indices[:6]}")

            self._ib = backend.create_buffer(index_data, usage="index")
            if self._ib and hasattr(self._ib, 'value') and self._ib.value:
                logger.info(f"[Mesh] Index buffer created: {hex(self._ib.value)}")
            else:
                logger.error(f"[Mesh] Failed to create index buffer for {self.name}")

        # 2️⃣ Привязываем буферы к пайплайну
        if self._vb:
            logger.info(f"[Mesh] Setting vertex buffers for {self.name}")
            backend.set_vertex_buffers(self._vb, self._ib)

            # 3️⃣ Выполняем draw‑call
            if self.indices is not None and self._ib:
                # index‑based draw
                logger.info(f"[Mesh] Drawing indexed: {len(self.indices)} indices")
                backend.draw_indexed(len(self.indices),
                                     start_index=0,
                                     base_vertex=0,
                                     instance_count=1)
                logger.info(f"[Mesh] Indexed draw completed")
            else:
                # простой non‑indexed draw
                logger.info(f"[Mesh] Drawing non-indexed: {len(self.vertices)} vertices")
                backend.draw(len(self.vertices),
                             start_vertex=0,
                             instance_count=1)
                logger.info(f"[Mesh] Non-indexed draw completed")
        else:
            logger.error(f"[Mesh] No vertex buffer for {self.name}")