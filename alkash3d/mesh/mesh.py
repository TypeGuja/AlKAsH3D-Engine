# alkash3d/scene/mesh.py
# -*- coding: utf-8 -*-
"""
Меш - геометрия сцены.
"""

from __future__ import annotations
import numpy as np
from alkash3d.scene.node import Node  # ✅ ВАЖНО: Импортируем Node


class Mesh(Node):  # ✅ КРИТИЧНО: Наследуемся от Node
    """
    Представляет геометрию в сцене.
    Содержит вершины, индексы и может быть отрисован рендерером.

    ✅ ИСПРАВЛЕНИЕ: Mesh теперь наследуется от Node и поддерживает traverse()
    """

    def __init__(self,
                 vertices: np.ndarray,
                 indices: np.ndarray | None = None,
                 normals: np.ndarray | None = None,
                 texcoords: np.ndarray | None = None):
        """
        Инициализирует меш.

        Args:
            vertices: Массив вершин (N, 3) в формате float32
            indices: Индексы для индексированной отрисовки (N,) - опционально
            normals: Нормали вершин (N, 3) - опционально
            texcoords: Текстурные координаты (N, 2) - опционально
        """
        # ✅ КРИТИЧНО: Вызываем конструктор родителя Node
        super().__init__()

        # Убеждаемся, что вершины - это numpy array
        self.vertices = np.asarray(vertices, dtype=np.float32)
        if self.vertices.ndim == 1:
            self.vertices = self.vertices.reshape(-1, 3)

        # Индексы для рисования
        self.indices = None
        if indices is not None:
            self.indices = np.asarray(indices, dtype=np.uint32)

        # Нормали (опционально)
        self.normals = None
        if normals is not None:
            self.normals = np.asarray(normals, dtype=np.float32)
            if self.normals.ndim == 1:
                self.normals = self.normals.reshape(-1, 3)

        # Текстурные координаты (опционально)
        self.texcoords = None
        if texcoords is not None:
            self.texcoords = np.asarray(texcoords, dtype=np.float32)
            if self.texcoords.ndim == 1:
                self.texcoords = self.texcoords.reshape(-1, 2)

        # Графические ресурсы (создаются рендерером)
        self.vertex_buffer = None
        self.index_buffer = None
        self.vao = None  # Vertex Array Object

        # Материал/цвет
        self.color = (1.0, 1.0, 1.0)  # RGB

        # Флаг видимости
        self.visible = True

    def get_vertex_count(self) -> int:
        """Возвращает количество вершин."""
        return len(self.vertices)

    def get_index_count(self) -> int:
        """Возвращает количество индексов (или вершин, если нет индексов)."""
        if self.indices is not None:
            return len(self.indices)
        return len(self.vertices)

    def has_indices(self) -> bool:
        """Проверяет, есть ли индексы."""
        return self.indices is not None

    def has_normals(self) -> bool:
        """Проверяет, есть ли нормали."""
        return self.normals is not None

    def has_texcoords(self) -> bool:
        """Проверяет, есть ли текстурные координаты."""
        return self.texcoords is not None