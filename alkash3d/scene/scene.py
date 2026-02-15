"""
Корневой узел сцены с поддержкой Octree‑culling.
"""

from alkash3d.scene.node import Node
from alkash3d.culling.octree import Octree

class Scene(Node):
    """Корневой узел сцены с поддержкой Octree‑culling."""
    def __init__(self):
        super().__init__("RootScene")
        self.culling = Octree(
            bounds=((-50.0, -50.0, -50.0), (50.0, 50.0, 50.0)),
            max_depth=6,
            max_objects=8,
        )

    def update(self, dt):
        for node in self.traverse():
            if hasattr(node, "on_update"):
                node.on_update(dt)
        self.culling.rebuild(self)

    def visible_nodes(self, camera):
        frustum = camera.get_view_projection_frustum()
        return self.culling.query(frustum)