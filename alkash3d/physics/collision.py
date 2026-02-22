# alkash3d/physics/collision.py
"""
Очень простая система обнаружения столкновений (только сфера‑сфера).
"""

from __future__ import annotations
from typing import List, Tuple, Any
from alkash3d.math.vec3 import Vec3


def detect_collisions(bodies: List[Any]) -> List[Tuple[Any, Any]]:
    """
    Возвращает список пар тел, у которых происходит столкновение.
    Тела считаются сферами, если ``shape`` содержит ``type='sphere'``
    и параметр ``radius``.
    """
    contacts: List[Tuple[Any, Any]] = []
    n = len(bodies)

    for i in range(n):
        a = bodies[i]
        if not a.shape or a.shape.get("type") != "sphere":
            continue
        for j in range(i + 1, n):
            b = bodies[j]
            if not b.shape or b.shape.get("type") != "sphere":
                continue

            # расстояние между центрами
            d = (a.position - b.position).length()
            rad_sum = a.shape.get("radius", 0.0) + b.shape.get("radius", 0.0)
            if d < rad_sum:
                contacts.append((a, b))

    return contacts
