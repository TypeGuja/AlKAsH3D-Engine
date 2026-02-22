# alkash3d/input/mouse.py
"""
Простейший объект‑мышь.
Хранит позицию курсора и набор нажатых кнопок.
"""
from __future__ import annotations
from typing import Set, Tuple


class Mouse:
    """Мышь – позиция и набор нажатых кнопок."""
    def __init__(self):
        self.x: float = 0.0
        self.y: float = 0.0
        self._buttons: Set[int] = set()

    # -----------------------------------------------------------------
    def move(self, x: float, y: float) -> None:
        self.x, self.y = x, y

    def button_down(self, button: int) -> None:
        self._buttons.add(button)

    def button_up(self, button: int) -> None:
        self._buttons.discard(button)

    def is_button_pressed(self, button: int) -> bool:
        return button in self._buttons

    def get_position(self) -> Tuple[float, float]:
        return (self.x, self.y)
