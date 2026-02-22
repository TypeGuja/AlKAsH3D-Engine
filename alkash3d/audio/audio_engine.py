# alkash3d/audio/audio_engine.py
"""
Минимальный аудио‑движок.
В реальном проекте здесь будет OpenAL/DirectSound и т.п.
Для тестов достаточно, чтобы класс существовал и мог хранить звуки.
"""

from __future__ import annotations
from typing import Dict
from .sound import Sound


class AudioEngine:
    """Хранит загруженные звуки и «воспроизводит» их (просто вывод в консоль)."""
    def __init__(self):
        self._sounds: Dict[str, Sound] = {}

    # -----------------------------------------------------------------
    def load(self, name: str, data: bytes | None = None) -> Sound:
        """Создаёт или возвращает уже загруженный звук."""
        if name not in self._sounds:
            self._sounds[name] = Sound(name, data)
        return self._sounds[name]

    # -----------------------------------------------------------------
    def play(self, name: str, loop: bool = False) -> None:
        """«Воспроизводит» звук – в этой упрощённой версии просто печать."""
        snd = self._sounds.get(name)
        if snd:
            print(f"[AudioEngine] Playing sound '{name}' (loop={loop})")
        else:
            raise RuntimeError(f"Sound '{name}' not loaded")
