# alkash3d/core/game.py
"""
Простейший «игровой» объект, содержащий набор сущностей.
"""
from __future__ import annotations
from typing import List, Any


class Game:
    """Контейнер для игровых объектов с простым обновлением."""
    def __init__(self):
        self.entities: List[Any] = []

    # -----------------------------------------------------------------
    def add_entity(self, entity: Any) -> None:
        """Добавить объект в игровой мир."""
        self.entities.append(entity)

    # -----------------------------------------------------------------
    def update(self, dt: float) -> None:
        """Вызвать у всех сущностей метод ``update(dt)``,
        если такой метод существует."""
        for e in self.entities:
            if hasattr(e, "update"):
                e.update(dt)
