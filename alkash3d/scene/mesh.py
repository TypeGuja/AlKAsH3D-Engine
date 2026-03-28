# alkash3d/scene/mesh.py
"""
Mesh – геометрический объект сцены.
ИСПРАВЛЕННАЯ ВЕРСИЯ с корректным разделением вершинного и индексного буферов
"""

import numpy as np
from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3
from typing import Optional
from alkash3d.utils import logger


class Mesh(Node):
    def __init__(self,
                 vertices: np.ndarray,
                 indices: Optional[np.ndarray] = None,
                 normals: Optional[np.ndarray] = None,
                 texcoords: Optional[np.ndarray] = None,
                 name: str = "Mesh"):
        super().__init__(name)

        # Приводим вершины к правильному формату
        self.vertices = vertices.astype(np.float32)
        if self.vertices.ndim == 1:
            self.vertices = self.vertices.reshape(-1, 3)

        # Нормали
        self.normals = normals.astype(np.float32) if normals is not None else None
        if self.normals is not None and self.normals.ndim == 1:
            self.normals = self.normals.reshape(-1, 3)

        # Текстурные координаты
        self.texcoords = texcoords.astype(np.float32) if texcoords is not None else None
        if self.texcoords is not None and self.texcoords.ndim == 1:
            self.texcoords = self.texcoords.reshape(-1, 2)

        # Индексы
        self.indices = indices.astype(np.uint32) if indices is not None else None
        if self.indices is not None and self.indices.ndim == 1:
            self.indices = self.indices.reshape(-1)

        # Количество вершин для отрисовки
        if self.indices is not None:
            self.vertex_count = len(self.indices)
        else:
            self.vertex_count = len(self.vertices)

        # Буферы GPU
        self._vb: Optional[any] = None  # Vertex buffer
        self._ib: Optional[any] = None  # Index buffer
        self._vb_created = False  # Флаг создания вершинного буфера
        self._ib_created = False  # Флаг создания индексного буфера

        self.visible = True
        self.color = Vec3(1.0, 1.0, 1.0)

        # Bounding sphere для culling
        if len(self.vertices) > 0:
            self._bounding_center = self.vertices.mean(axis=0)
            distances = np.linalg.norm(self.vertices - self._bounding_center, axis=1)
            self._bounding_radius = float(np.max(distances))
        else:
            self._bounding_center = np.array([0, 0, 0], dtype=np.float32)
            self._bounding_radius = 1.0

        logger.info(f"[Mesh] Created '{name}': {len(self.vertices)} vertices, "
                    f"{len(self.indices) if self.indices is not None else 0} indices")

    @property
    def bounding_sphere(self):
        return self._bounding_center, self._bounding_radius

    def _create_vertex_buffer(self, backend):
        """Создаёт вершинный буфер."""
        if self._vb_created:
            return True

        if len(self.vertices) == 0:
            logger.error(f"[Mesh] No vertices for '{self.name}'")
            return False

        vertex_data = self.vertices.tobytes()
        logger.debug(f"[Mesh] Creating vertex buffer for '{self.name}', size={len(vertex_data)} bytes")

        self._vb = backend.create_buffer(vertex_data, usage="vertex")

        if not self._vb or not getattr(self._vb, 'value', 0):
            logger.error(f"[Mesh] Failed to create vertex buffer for '{self.name}'")
            return False

        self._vb_created = True
        logger.debug(f"[Mesh] Vertex buffer created at 0x{self._vb.value:X}")
        return True

    def _create_index_buffer(self, backend):
        """Создаёт индексный буфер (только если есть индексы)."""
        if self._ib_created:
            return True

        if self.indices is None or len(self.indices) == 0:
            return False

        index_data = self.indices.tobytes()
        logger.debug(f"[Mesh] Creating index buffer for '{self.name}', size={len(index_data)} bytes")

        self._ib = backend.create_buffer(index_data, usage="index")

        if not self._ib or not getattr(self._ib, 'value', 0):
            logger.error(f"[Mesh] Failed to create index buffer for '{self.name}'")
            return False

        self._ib_created = True
        logger.debug(f"[Mesh] Index buffer created at 0x{self._ib.value:X}")
        return True

    def draw(self, backend):
        """Отрисовка меша."""
        if not self.visible:
            return

        # Создаём вершинный буфер
        if not self._create_vertex_buffer(backend):
            logger.error(f"[Mesh] Cannot draw '{self.name}' - no vertex buffer")
            return

        # Проверяем, что вершинный буфер валиден
        vb_val = self._vb.value if hasattr(self._vb, 'value') else int(self._vb)
        if vb_val == 0:
            logger.error(f"[Mesh] Vertex buffer invalid for '{self.name}'")
            return

        # Создаём индексный буфер, если есть индексы
        has_indices = self.indices is not None and len(self.indices) > 0
        ib_ptr = None

        if has_indices:
            if not self._create_index_buffer(backend):
                logger.error(f"[Mesh] Cannot draw '{self.name}' - failed to create index buffer")
                return

            ib_val = self._ib.value if hasattr(self._ib, 'value') else int(self._ib)
            if ib_val == 0:
                logger.error(f"[Mesh] Index buffer invalid for '{self.name}'")
                return

            # КРИТИЧЕСКАЯ ПРОВЕРКА: вершинный и индексный буферы не должны совпадать
            if vb_val == ib_val:
                logger.error(f"[Mesh] CRITICAL: Vertex and index buffers have same address "
                             f"0x{vb_val:X} for '{self.name}'")
                return

            ib_ptr = self._ib

        # Привязываем буферы
        try:
            logger.debug(f"[Mesh] Setting buffers for '{self.name}': vb=0x{vb_val:X}, "
                         f"ib=0x{ib_ptr.value:X if ib_ptr else 0}")

            result = backend.set_vertex_buffers(self._vb, ib_ptr)
            if not result:
                logger.error(f"[Mesh] set_vertex_buffers failed for '{self.name}'")
                return
        except Exception as e:
            logger.error(f"[Mesh] set_vertex_buffers exception for '{self.name}': {e}")
            return

        # Выполняем draw call
        try:
            if has_indices and self._ib:
                logger.debug(f"[Mesh] Drawing indexed: {self.vertex_count} indices")
                backend.draw_indexed(self.vertex_count, start_index=0, base_vertex=0, instance_count=1)
            else:
                logger.debug(f"[Mesh] Drawing non-indexed: {self.vertex_count} vertices")
                backend.draw(self.vertex_count, start_vertex=0, instance_count=1)
        except Exception as e:
            logger.error(f"[Mesh] Draw call failed for '{self.name}': {e}")

    def __repr__(self) -> str:
        return f"Mesh('{self.name}', vertices={len(self.vertices)}, indices={len(self.indices) if self.indices else 0})"