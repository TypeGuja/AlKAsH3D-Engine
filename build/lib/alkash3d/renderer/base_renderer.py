# -*- coding: utf-8 -*-
"""Абстрактный базовый рендерер."""
from abc import ABC, abstractmethod

class BaseRenderer(ABC):
    @abstractmethod
    def render(self, scene, camera):
        """Отрисовать один кадр."""
        pass

    @abstractmethod
    def resize(self, w, h):
        """Обновить размеры viewport / framebuffers."""
        pass
