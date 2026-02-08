# -*- coding: utf-8 -*-
"""Базовый узел графа сцены."""
import numpy as np
from alkash3d.math.vec3 import Vec3
from alkash3d.math.mat4 import Mat4

class Node:
    """Все элементы сцены наследуются от Node."""
    def __init__(self, name="Node"):
        self.name = name
        self.children = []
        self.parent = None
        self.position = Vec3()
        self.rotation = Vec3()   # Эйлеровы углы в градусах (можно заменить quaternion)
        self.scale = Vec3(1.0, 1.0, 1.0)

    # ----------------- трансформации -----------------
    def get_local_matrix(self):
        """model = T * R * S."""
        T = Mat4.translate(self.position.x, self.position.y, self.position.z)
        R = Mat4.from_euler(self.rotation.x,
                             self.rotation.y,
                             self.rotation.z)
        S = Mat4.scale(self.scale.x, self.scale.y, self.scale.z)
        return T @ R @ S

    def get_world_matrix(self):
        """Рекурсивный обход к родителю."""
        if self.parent is None:
            return self.get_local_matrix()
        else:
            return self.parent.get_world_matrix() @ self.get_local_matrix()

    # ----------------- иерархия -----------------
    def add_child(self, node):
        node.parent = self
        self.children.append(node)

    def remove_child(self, node):
        if node in self.children:
            node.parent = None
            self.children.remove(node)

    def traverse(self):
        """Генератор DFS."""
        yield self
        for child in self.children:
            yield from child.traverse()
