# File: alkash3d/scene/mesh.py
"""
Примитивный объект – создаёт буферы в GPU‑драйвере при первом draw().
Поддерживает отдельные массивы позиций, нормалей и texcoords.
"""

import numpy as np
from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3


class Mesh(Node):
    """
    Примитивный объект сцены.

    * `vertices`  – 1‑D массив float32, **только позиции** (x, y, z, …).
    * `normals`  – (необязательно) 1‑D массив float32, **только нормали**.
    * `texcoords`– (необязательно) 1‑D массив float32, **UV** (u, v).
    * `indices`  – (необязательно) 1‑D массив uint32.
    """

    def __init__(
        self,
        vertices: np.ndarray,
        normals: np.ndarray | None = None,
        texcoords: np.ndarray | None = None,
        indices: np.ndarray | None = None,
        name: str = "Mesh",
    ) -> None:
        super().__init__(name)

        # ------------------------------------------------------------------
        # Храним массивы «как есть» – `Mesh` сам потом упакует их в один
        # interleaved‑буфер, если это понадобится (draw()).
        # ------------------------------------------------------------------
        self.vertices = np.asarray(vertices, dtype=np.float32)
        self.normals = (
            None if normals is None else np.asarray(normals, dtype=np.float32)
        )
        self.texcoords = (
            None if texcoords is None else np.asarray(texcoords, dtype=np.float32)
        )
        self.indices = (
            None if indices is None else np.asarray(indices, dtype=np.uint32)
        )

        # ------------------------------------------------------------------
        # Индексный/вершинный счётчик (нужен в draw())
        # ------------------------------------------------------------------
        self.index_count = (
            len(self.indices) if self.indices is not None else len(self.vertices) // 3
        )

        # ------------------------------------------------------------------
        # Простейший материал – просто цвет (можно переопределить позже)
        # ------------------------------------------------------------------
        self.color = Vec3(1.0, 1.0, 1.0)

        # ------------------------------------------------------------------
        # Вычисляем bounding‑sphere (для culling)
        # ------------------------------------------------------------------
        verts = self.vertices
        if verts.ndim == 1:
            verts = verts.reshape((-1, 3))
        self._bounding_center = verts.mean(axis=0).astype(np.float32)
        self._bounding_radius = np.linalg.norm(verts - self._bounding_center, axis=1).max()

        # ------------------------------------------------------------------
        # GPU‑буферы – создаются лениво в первом draw()
        # ------------------------------------------------------------------
        self._vertex_buffer = None
        self._index_buffer = None

    # ----------------------------------------------------------------------
    # Внутренний helper – упаковывает данные в interleaved‑формат:
    #   [pos][norm][uv]… (если нормали/UV заданы)
    # ----------------------------------------------------------------------
    def _interleave(self) -> np.ndarray:
        components = [self.vertices]
        if self.normals is not None:
            components.append(self.normals)
        if self.texcoords is not None:
            components.append(self.texcoords)

        # column‑stack → interleaved, потом flatten → 1‑D массив float32
        interleaved = np.column_stack(components).astype(np.float32).ravel()
        return interleaved

    # ----------------------------------------------------------------------
    # Создаём GPU‑буферы (вызывается один раз в draw())
    # ----------------------------------------------------------------------
    def _setup_gpu_buffers(self, backend) -> None:
        # Vertex buffer (UPLOAD‑heap → copy в DEFAULT‑heap внутри backend)
        interleaved = self._interleave()
        self._vertex_buffer = backend.create_buffer(interleaved.tobytes(), usage="vertex")

        # Индексный буфер – если есть
        if self.indices is not None:
            self._index_buffer = backend.create_buffer(self.indices.tobytes(), usage="index")
        else:
            self._index_buffer = None

    # ----------------------------------------------------------------------
    # Отрисовка
    # ----------------------------------------------------------------------
    def draw(self, backend) -> None:
        """Отрисовать меш, создавая GPU‑буферы «лениво»."""
        if self._vertex_buffer is None:
            self._setup_gpu_buffers(backend)

        backend.set_vertex_buffers(self._vertex_buffer, self._index_buffer)

        # Если есть индекс‑буфер – используем draw_indexed,
        # иначе – обычный draw.
        if self._index_buffer is not None:
            backend.draw_indexed(self.index_count)
        else:
            backend.draw(self.index_count)

    # ----------------------------------------------------------------------
    # Bounding‑sphere в мировых координатах (для кулинга)
    # ----------------------------------------------------------------------
    @property
    def bounding_sphere(self) -> tuple[np.ndarray, float]:
        """
        (центр, радиус) в мировых координатах.
        """
        world = self.get_world_matrix().to_np()
        centre_h = np.append(self._bounding_center, 1.0).astype(np.float32)
        centre_world = world @ centre_h
        scale = np.linalg.norm(world[0:3, 0:3], axis=0).max()
        return centre_world[:3], float(self._bounding_radius * scale)