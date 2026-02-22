# alkash3d/audio/sound.py
"""
Простейший звуковой объект.
"""
from __future__ import annotations
from alkash3d.math.vec3 import Vec3


class Sound:
    """Звук с базовыми параметрами."""
    def __init__(self, name: str = "", data: bytes | None = None):
        self.name = name
        self.data = data
        self.volume: float = 1.0
        self.pitch: float = 1.0
        self.looping: bool = False
        self.position: Vec3 = Vec3()
        self.velocity: Vec3 = Vec3()

    # -----------------------------------------------------------------
    def set_position(self, x: float, y: float, z: float) -> None:
        self.position = Vec3(x, y, z)

    def set_velocity(self, x: float, y: float, z: float) -> None:
        self.velocity = Vec3(x, y, z)

    def __repr__(self) -> str:
        return f"<Sound name={self.name} vol={self.volume} pitch={self.pitch}>"
