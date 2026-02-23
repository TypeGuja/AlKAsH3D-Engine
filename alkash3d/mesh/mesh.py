# alkash3d/scene/mesh.py
# -*- coding: utf-8 -*-
"""
Mesh – геометрический объект сцены.
"""

from __future__ import annotations
import numpy as np
from alkash3d.scene.node import Node  # ✅ Важный импорт

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
        self._vb = None          # vertex buffer (ctypes.c_void_p)
        self._ib = None          # index buffer (ctypes.c_void_p) – может быть None

        # Видимость – можно отключать объектом
        self.visible = True

    # -----------------------------------------------------------------
    # Простейший draw‑метод, совместимый с DX12Backend
    # -----------------------------------------------------------------
    def draw(self, backend):
        """
        Создаёт (при первом вызове) vertex‑/index‑буферы,
        привязывает их к командному списку и отрисовывает.
        """
        # 1️⃣ Сначала создаём буферы, если их ещё нет
        if self._vb is None:
            # vertices → raw bytes, layout 3×float32 = 12 байт
            self._vb = backend.create_buffer(self.vertices.tobytes(),
                                             usage="vertex")
        if self.indices is not None and self._ib is None:
            self._ib = backend.create_buffer(self.indices.tobytes(),
                                             usage="index")

        # 2️⃣ Привязываем буферы к пайплайну
        backend.set_vertex_buffers(self._vb, self._ib)

        # 3️⃣ Выполняем draw‑call
        if self.indices is not None:
            # index‑based draw
            backend.draw_indexed(self.indices.size,
                                 start_index=0,
                                 base_vertex=0,
                                 instance_count=1)
        else:
            # простой non‑indexed draw
            backend.draw(self.vertices.shape[0],
                         start_vertex=0,
                         instance_count=1)
