# alkash3d/physics/rigid_body.py
"""
Простейшее твердое тело – хранит массу, позицию, скорость и накопленную силу.
"""

from __future__ import annotations
from alkash3d.math.vec3 import Vec3
from typing import Dict, Any


class RigidBody:
    """Твердое тело с простым физическим описанием."""
    def __init__(self,
                 mass: float = 1.0,
                 position: Vec3 | None = None,
                 velocity: Vec3 | None = None,
                 shape: Dict[str, Any] | None = None):
        self.mass: float = mass
        self.position: Vec3 = position or Vec3()
        self.velocity: Vec3 = velocity or Vec3()
        self.force: Vec3 = Vec3()
        # Описание формы для простых коллизий (например, {"type":"sphere","radius":1.0})
        self.shape: Dict[str, Any] | None = shape

    # -----------------------------------------------------------------
    def apply_force(self, f: Vec3) -> None:
        """Накладывает силу (суммируется до следующего шага)."""
        self.force = self.force + f

    def __repr__(self) -> str:
        return (f"<RigidBody mass={self.mass} pos={self.position} "
                f"vel={self.velocity}>")
