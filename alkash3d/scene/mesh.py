"""
Примитивный объект – создаёт буферы в GPU‑драйвере при первом draw().
"""

import numpy as np
from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3

class Mesh(Node):
    """Примитивный объект – создаёт буферы в GPU‑драйвере при первом draw()."""
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

        self.vb = None
        self.ib = None
        self.index_count = len(self.indices) if self.indices is not None else len(self.vertices) // 3
        self.color = Vec3(1.0, 1.0, 1.0)

        # bounding sphere
        verts = self.vertices
        if verts.ndim == 1:
            verts = verts.reshape((-1, 3))
        self._bounding_center = verts.mean(axis=0).astype(np.float32)
        self._bounding_radius = np.linalg.norm(verts - self._bounding_center, axis=1).max()

    def _setup_gpu_buffers(self, backend):
        components = [self.vertices]
        if self.normals is not None:
            components.append(self.normals)
        if self.texcoords is not None:
            components.append(self.texcoords)
        interleaved = np.column_stack(components).astype(np.float32).ravel()
        self.vb = backend.create_buffer(interleaved.tobytes(), usage="vertex")
        if self.indices is not None:
            self.ib = backend.create_buffer(self.indices.tobytes(), usage="index")
        else:
            self.ib = None

    def draw(self, backend):
        """Отрисовать меш, создавая буферы «лениво»."""
        if self.vb is None:
            self._setup_gpu_buffers(backend)

        backend.set_vertex_buffers(self.vb, self.ib)
        if self.ib is not None:
            backend.draw_indexed(self.index_count)
        else:
            backend.draw(self.index_count)

    @property
    def bounding_sphere(self):
        """(центр, радиус) в мировых координатах."""
        world = self.get_world_matrix().to_np()
        centre_h = np.append(self._bounding_center, 1.0).astype(np.float32)
        centre_world = world @ centre_h
        scale = np.linalg.norm(world[0:3, 0:3], axis=0).max()
        return centre_world[:3], float(self._bounding_radius * scale)