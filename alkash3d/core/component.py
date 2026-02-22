# alkash3d/core/component.py
"""
Базовый компонент для системы «entity‑component».
В реальном движке будет хранить ссылки, события и т.п.
"""
from __future__ import annotations

from typing import Any


class Component:
    """Базовый компонент – хранит ссылку на владельца."""
    def __init__(self):
        self.owner = None

    def attach(self, owner: Any) -> None:
        """Привязать компонент к объекту."""
        self.owner = owner

    def detach(self) -> None:
        """Отвязать от владельца."""
        self.owner = None
