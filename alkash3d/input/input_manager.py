# alkash3d/input/input_manager.py
"""
Минимальный менеджер ввода.
Если передан ``window`` (handle GLFW), методы используют реальные вызовы.
Если окна нет – работает в «симуляционном» режиме: состояния можно
установить вручную через ``set_key``.
"""
from __future__ import annotations
import glfw
from typing import Dict, Any


class InputManager:
    """Отслеживает состояние клавиш и кнопок мыши."""
    def __init__(self, window: Any = None):
        self.window = window
        # собственный словарь состояний, если окна нет
        self._key_state: Dict[int, bool] = {}

    # -----------------------------------------------------------------
    def is_key_pressed(self, key: int) -> bool:
        """Вернуть True, если клавиша ``key`` нажата."""
        if self.window:
            return glfw.get_key(self.window, key) == glfw.PRESS
        return self._key_state.get(key, False)

    # -----------------------------------------------------------------
    def set_key(self, key: int, pressed: bool) -> None:
        """Установить состояние клавиши вручную (симуляция)."""
        self._key_state[key] = pressed

    # -----------------------------------------------------------------
    def bind_action(self, name: str, key: int) -> None:
        """Привязать «логическое действие``name`` к клавише ``key``."""
        # В этой упрощённой версии просто сохраняем состояние в словарь
        self._key_state[name] = self.is_key_pressed(key)

    # -----------------------------------------------------------------
    # Дополнительные методы, часто используемые в примерах:
    def get_mouse_position(self) -> tuple[int, int]:
        if self.window:
            return glfw.get_cursor_pos(self.window)
        return (0, 0)

    def is_mouse_button_pressed(self, button: int) -> bool:
        if self.window:
            return glfw.get_mouse_button(self.window, button) == glfw.PRESS
        return False
