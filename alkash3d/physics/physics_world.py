# alkash3d/physics/physics_world.py
"""
Простейший мир физики с гравитацией и интеграцией твердого тела.
"""

from __future__ import annotations
from typing import List
from alkash3d.math.vec3 import Vec3
from .rigid_body import RigidBody
from .collision import detect_collisions


class PhysicsWorld:
    """Симуляция физики – шаг за шагом."""
    def __init__(self, gravity: Vec3 | None = None):
        self.gravity: Vec3 = gravity or Vec3(0.0, -9.81, 0.0)
        self.bodies: List[RigidBody] = []

    # -----------------------------------------------------------------
    def add_body(self, body: RigidBody) -> None:
        """Добавить твердое тело в мир."""
        self.bodies.append(body)

    # -----------------------------------------------------------------
    def step(self, dt: float) -> None:
        """Выполнить один шаг симуляции (прямой Эйлер)."""
        for body in self.bodies:
            if body.mass == 0:
                continue          # бесконечно тяжёлый объект – не двигаем

            # a = (gravity + force/mass)
            acceleration = self.gravity + (body.force * (1.0 / body.mass))
            # интегрируем скорость и позицию
            body.velocity = body.velocity + acceleration * dt
            body.position = body.position + body.velocity * dt
            # сбрасываем накопленную силу
            body.force = Vec3()

        # простая проверка столкновений – только для демонстрации
        detect_collisions(self.bodies)
