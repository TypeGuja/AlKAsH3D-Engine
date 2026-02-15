# -*- coding: utf-8 -*-
"""
Абстрактный базовый рендерер.
"""

from abc import ABC, abstractmethod

class BaseRenderer(ABC):
    @abstractmethod
    def render(self, scene, camera) -> None:
        """Отрисовать один кадр."""
        pass

    @abstractmethod
    def resize(self, w: int, h: int) -> None:
        """Обновить размер viewport / framebuffer."""
        pass
