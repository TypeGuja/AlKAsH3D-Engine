# alkash3d/mesh/mesh.py
import numpy as np
from OpenGL import GL
from alkash3d.scene.node import Node
from alkash3d.utils import logger
from alkash3d.math.vec3 import Vec3


class Mesh(Node):
    """Примитивный объект – лениво создаёт VAO при первом draw()."""

    def __init__(self,
                 vertices: np.ndarray,
                 normals: np.ndarray = None,
                 texcoords: np.ndarray = None,
                 indices: np.ndarray = None,
                 name="Mesh"):
        super().__init__(name)

        self.vertices = vertices.astype(np.float32)
        self.normals = normals.astype(np.float32) if normals is not None else None
        self.texcoords = texcoords.astype(np.float32) if texcoords is not None else None
        self.indices = indices.astype(np.uint32) if indices is not None else None

        self.vao = None
        self.vbo = None
        self.nbo = None
        self.tbo = None
        self.ebo = None

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
    def _setup_vao(self):
        self.vao = GL.glGenVertexArrays(1)
        GL.glBindVertexArray(self.vao)

        # позиции
        self.vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.vbo)
        GL.glBufferData(GL.GL_ARRAY_BUFFER,
                        self.vertices.nbytes,
                        self.vertices,
                        GL.GL_STATIC_DRAW)
        GL.glEnableVertexAttribArray(0)
        GL.glVertexAttribPointer(0, 3, GL.GL_FLOAT, False, 0, None)

        # нормали
        if self.normals is not None:
            self.nbo = GL.glGenBuffers(1)
            GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.nbo)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,
                            self.normals.nbytes,
                            self.normals,
                            GL.GL_STATIC_DRAW)
            GL.glEnableVertexAttribArray(1)
            GL.glVertexAttribPointer(1, 3, GL.GL_FLOAT, False, 0, None)

        # texcoords
        if self.texcoords is not None:
            self.tbo = GL.glGenBuffers(1)
            GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.tbo)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,
                            self.texcoords.nbytes,
                            self.texcoords,
                            GL.GL_STATIC_DRAW)
            GL.glEnableVertexAttribArray(2)
            GL.glVertexAttribPointer(2, 2, GL.GL_FLOAT, False, 0, None)

        # индексы
        if self.indices is not None:
            self.ebo = GL.glGenBuffers(1)
            GL.glBindBuffer(GL.GL_ELEMENT_ARRAY_BUFFER, self.ebo)
            GL.glBufferData(GL.GL_ELEMENT_ARRAY_BUFFER,
                            self.indices.nbytes,
                            self.indices,
                            GL.GL_STATIC_DRAW)

        GL.glBindVertexArray(0)

    # -----------------------------------------------------------------
    def _ensure_vao(self):
        if self.vao is None:
            self._setup_vao()

    # -----------------------------------------------------------------
    def draw(self):
        """Отрисовка через чистый OpenGL."""
        self._ensure_vao()
        GL.glBindVertexArray(self.vao)

        if self.indices is not None:
            GL.glDrawElements(GL.GL_TRIANGLES, self.index_count,
                              GL.GL_UNSIGNED_INT, None)
        else:
            GL.glDrawArrays(GL.GL_TRIANGLES, 0, self.index_count)

        GL.glBindVertexArray(0)

    # -----------------------------------------------------------------
    @property
    def bounding_sphere(self) -> tuple[np.ndarray, float]:
        """(центр, радиус) в мировых координатах."""
        world = self.get_world_matrix().to_np()
        centre_h = np.append(self._bounding_center, 1.0).astype(np.float32)
        centre_world = world @ centre_h
        scale = np.linalg.norm(world[0:3, 0:3], axis=0).max()
        return centre_world[:3], float(self._bounding_radius * scale)

    # -----------------------------------------------------------------
    def cleanup(self):
        """Освободить GL‑ресурсы."""
        try:
            if self.vao:
                GL.glDeleteVertexArrays(1, [self.vao])
            if self.vbo:
                GL.glDeleteBuffers(1, [self.vbo])
            if self.nbo:
                GL.glDeleteBuffers(1, [self.nbo])
            if self.tbo:
                GL.glDeleteBuffers(1, [self.tbo])
            if self.ebo:
                GL.glDeleteBuffers(1, [self.ebo])
        except Exception as exc:
            logger.debug(f"Mesh.cleanup(): {exc}")

    def __del__(self):
        self.cleanup()