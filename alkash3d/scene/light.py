"""
Light classes for the scene graph.
"""

from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3


class Light(Node):
    """Base class for all lights."""
    def __init__(self, name="Light"):
        super().__init__(name)
        self.color = Vec3(1.0, 1.0, 1.0)
        self.intensity = 1.0


class DirectionalLight(Light):
    """Directional light (sun-like)."""
    def __init__(self, direction=Vec3(0.0, -1.0, 0.0), color=Vec3(1.0, 1.0, 1.0), intensity=1.0):
        super().__init__("DirectionalLight")
        # ИСПРАВЛЕНО: .normalized() -> .normalize()
        self.direction = direction.normalize()
        self.color = color
        self.intensity = intensity


class PointLight(Light):
    """Point light (omni-directional)."""
    def __init__(self, position=Vec3(0.0, 0.0, 0.0), color=Vec3(1.0, 1.0, 1.0), intensity=1.0):
        super().__init__("PointLight")
        self.position = position
        self.color = color
        self.intensity = intensity


class SpotLight(Light):
    """Spot light (cone-shaped)."""
    def __init__(self, position=Vec3(0.0, 0.0, 0.0), direction=Vec3(0.0, -1.0, 0.0),
                 color=Vec3(1.0, 1.0, 1.0), intensity=1.0, angle=45.0):
        super().__init__("SpotLight")
        self.position = position
        # ИСПРАВЛЕНО: .normalized() -> .normalize()
        self.direction = direction.normalize()
        self.color = color
        self.intensity = intensity
        self.angle = angle