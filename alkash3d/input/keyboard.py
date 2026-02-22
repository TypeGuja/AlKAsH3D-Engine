# alkash3d/input/keyboard.py
"""
Простейший объект‑клавиатура.
Внутри хранит множество «нажатых» клавиш.
"""
from __future__ import annotations
from typing import Set


class Keyboard:
    """Клавиатура – набор текущих нажатий."""
    def __init__(self):
        self._pressed: Set[int] = set()

    # -----------------------------------------------------------------
    def press(self, key: int) -> None:
        self._pressed.add(key)

    def release(self, key: int) -> None:
        self._pressed.discard(key)

    def is_pressed(self, key: int) -> bool:
        return key in self._pressed
