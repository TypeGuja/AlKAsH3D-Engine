# alkas3d/scene/light.py
# ---------------------------------------------------------------
# Базовые типы освещения: Directional, Point и Spot.
# Все они – наследники Node, поэтому могут быть вложены в иерархию
# (например, перемещать точечный свет вместе с объектом).
# ---------------------------------------------------------------

from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3
import numpy as np

class Light(Node):
    """Базовый класс для всех видов света."""
    def __init__(self, color: Vec3 = Vec3(1.0, 1.0, 1.0), intensity: float = 1.0, name="Light"):
        super().__init__(name)
        self.color = color
        self.intensity = float(intensity)

    def get_uniforms(self) -> dict:
        """Возврат словаря, который будет передан в шейдер."""
        raise NotImplementedError

class DirectionalLight(Light):
    """Свет из бесконечности, задаётся направлением."""
    def __init__(self, direction: Vec3 = Vec3(0, -1, 0), **kwargs):
        super().__init__(**kwargs)
        self.direction = direction.normalized()

    def get_uniforms(self):
        return {
            "type": 0,                         # 0 = directional
            "direction": self.direction.as_np(),
            "color": self.color.as_np(),
            "intensity": self.intensity,
        }

class PointLight(Light):
    """Точечный свет с радиусом затухания."""
    def __init__(self, radius: float = 10.0, **kwargs):
        super().__init__(**kwargs)
        self.radius = float(radius)

    def get_uniforms(self):
        return {
            "type": 1,                         # 1.txt = point
            "position": self.get_world_position().as_np(),
            "color": self.color.as_np(),
            "intensity": self.intensity,
            "radius": self.radius,
        }

    def get_world_position(self):
        """Извлекаем позицию из world‑матрицы (транслирование)."""
        world = self.get_world_matrix()
        return Vec3(world[0, 3], world[1, 3], world[2, 3])

class SpotLight(Light):
    """Прожектор (точечный + угол)."""
    def __init__(self, direction: Vec3 = Vec3(0, -1, 0),
                 inner_angle: float = 15.0,
                 outer_angle: float = 30.0,
                 **kwargs):
        super().__init__(**kwargs)
        self.direction = direction.normalized()
        self.inner_angle = float(inner_angle)   # degrees
        self.outer_angle = float(outer_angle) # degrees

    def get_uniforms(self):
        return {
            "type": 2,   # 2 = spot
            "position": self.get_world_position().as_np(),
            "direction": self.direction.as_np(),
            "color": self.color.as_np(),
            "intensity": self.intensity,
            "innerCutoff": np.cos(np.radians(self.inner_angle)),
            "outerCutoff": np.cos(np.radians(self.outer_angle)),
        }

    def get_world_position(self):
        world = self.get_world_matrix()
        return Vec3(world[0, 3], world[1, 3], world[2, 3])
