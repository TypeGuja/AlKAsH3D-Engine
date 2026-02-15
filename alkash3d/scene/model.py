"""
Объединяет несколько Mesh‑ов в одну модель.
"""

from alkash3d.scene.node import Node

class Model(Node):
    """Объединяет несколько Mesh‑ов в один объект‑модель."""
    def __init__(self, meshes, name="Model"):
        super().__init__(name)
        self.meshes = meshes
        for m in meshes:
            self.add_child(m)

    def draw(self):
        for m in self.meshes:
            m.draw(self.backend)   # backend будет передан из рендера
