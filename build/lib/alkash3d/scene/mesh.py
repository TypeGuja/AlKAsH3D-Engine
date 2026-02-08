# alkas3d/scene/mesh.py
# ---------------------------------------------------------------
# Mesh – геометрический объект, содержит VBO/VAO.
# VAO создаётся «лениво», только при первом draw().
# ---------------------------------------------------------------
import numpy as np
from OpenGL import GL
from alkash3d.scene.node import Node


class Mesh(Node):
    """Загружает OBJ‑модель (упрощенно) и создаёт VAO только при первом draw()."""

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

        # Пока VAO не создаётся – будем создавать в первом draw()
        self.vao = None
        self.vbo = None
        self.nbo = None
        self.tbo = None
        self.ebo = None

        if self.indices is not None:
            self.index_count = len(self.indices)
        else:
            self.index_count = len(self.vertices) // 3

    # -----------------------------------------------------------------
    # Создание VAO (вызывается один раз, когда уже есть GL‑контекст)
    # -----------------------------------------------------------------
    def _setup_vao(self):
        self.vao = GL.glGenVertexArrays(1)
        GL.glBindVertexArray(self.vao)

        # VBO – позиции
        self.vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.vbo)
        GL.glBufferData(GL.GL_ARRAY_BUFFER,
                        self.vertices.nbytes,
                        self.vertices,
                        GL.GL_STATIC_DRAW)
        GL.glEnableVertexAttribArray(0)               # layout(location = 0)
        GL.glVertexAttribPointer(0, 3, GL.GL_FLOAT, False, 0, None)

        # Нормали
        if self.normals is not None:
            self.nbo = GL.glGenBuffers(1)
            GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.nbo)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,
                            self.normals.nbytes,
                            self.normals,
                            GL.GL_STATIC_DRAW)
            GL.glEnableVertexAttribArray(1)           # layout(location = 1)
            GL.glVertexAttribPointer(1, 3, GL.GL_FLOAT, False, 0, None)

        # Текстурные координаты
        if self.texcoords is not None:
            self.tbo = GL.glGenBuffers(1)
            GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.tbo)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,
                            self.texcoords.nbytes,
                            self.texcoords,
                            GL.GL_STATIC_DRAW)
            GL.glEnableVertexAttribArray(2)           # layout(location = 2)
            GL.glVertexAttribPointer(2, 2, GL.GL_FLOAT, False, 0, None)

        # Индексы (если есть)
        if self.indices is not None:
            self.ebo = GL.glGenBuffers(1)
            GL.glBindBuffer(GL.GL_ELEMENT_ARRAY_BUFFER, self.ebo)
            GL.glBufferData(GL.GL_ELEMENT_ARRAY_BUFFER,
                            self.indices.nbytes,
                            self.indices,
                            GL.GL_STATIC_DRAW)

        GL.glBindVertexArray(0)   # отвязать

    # -----------------------------------------------------------------
    # Убедиться, что VAO готов
    # -----------------------------------------------------------------
    def _ensure_vao(self):
        if self.vao is None:
            self._setup_vao()

    # -----------------------------------------------------------------
    # Рендер
    # -----------------------------------------------------------------
    def draw(self):
        self._ensure_vao()
        GL.glBindVertexArray(self.vao)

        if self.indices is not None:
            GL.glDrawElements(GL.GL_TRIANGLES,
                              self.index_count,
                              GL.GL_UNSIGNED_INT,
                              None)
        else:
            GL.glDrawArrays(GL.GL_TRIANGLES,
                            0,
                            self.index_count)

        GL.glBindVertexArray(0)

    # -----------------------------------------------------------------
    # Модель‑матрица для шейдера
    # -----------------------------------------------------------------
    def get_model_matrix(self):
        """Возвратить world‑matrix как numpy‑массив 4×4."""
        from ..math.mat4 import Mat4
        return self.get_world_matrix().to_np()