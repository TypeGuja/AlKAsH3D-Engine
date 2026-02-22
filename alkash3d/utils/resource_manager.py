# alkash3d/utils/resource_manager.py
"""
Простейший менеджер кэширования произвольных ресурсов.
Используется в тестах только для проверки, что модуль импортируется.
"""

from __future__ import annotations
from typing import Callable, Any, Dict


class ResourceManager:
    """Кеширующий менеджер ресурсов.

    Пример:
        rm = ResourceManager()
        tex = rm.load("grass", load_texture, path, backend)
    """
    def __init__(self):
        self._cache: Dict[str, Any] = {}

    def load(self, key: str, loader: Callable[..., Any], *args, **kwargs) -> Any:
        """Загружает ресурс через ``loader`` и кеширует его.

        Если ресурс уже в кешe – возвращается сохранённый объект,
        иначе вызывается ``loader(*args, **kwargs)``.
        """
        if key in self._cache:
            return self._cache[key]

        resource = loader(*args, **kwargs)
        self._cache[key] = resource
        return resource

    def clear(self) -> None:
        """Очистить кеш."""
        self._cache.clear()

    def __contains__(self, key: str) -> bool:
        return key in self._cache
