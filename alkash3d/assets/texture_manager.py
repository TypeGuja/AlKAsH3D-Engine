# alkash3d/assets/texture_manager.py
"""Менеджер кэширования текстур – DX12‑совместимый."""

from alkash3d.utils.logger import logger
from alkash3d.utils.texture_loader import load_texture


class TextureManager:
    """Кеширующий менеджер текстур – один объект на процесс."""
    _cache = {}

    @classmethod
    def get(cls, path: str, backend):
        if path in cls._cache:
            return cls._cache[path]
        tex = load_texture(path, backend)   # создаёт DX12‑текстуру
        cls._cache[path] = tex
        logger.debug(f"[TextureManager] Loaded texture: {path}")
        return tex
