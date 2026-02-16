# alkash3d/window.py
"""
Окно + GLFW‑контекст (без OpenGL‑контекста, нужен для DX12).
"""

import glfw
import os
from alkash3d.core.input import InputManager
from pathlib import Path
from typing import Optional

class Window:
    """Окно + GLFW‑контекст."""
    def __init__(self, width: int = 1280, height: int = 720, title: str = "AlKAsH3D Engine"):
        if not glfw.init():
            raise RuntimeError("Failed to initialize GLFW")

        # Мы используем только WinAPI‑контекст, OpenGL‑контекст не нужен.
        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)

        self.handle = glfw.create_window(width, height, title, None, None)
        if not self.handle:
            glfw.terminate()
            raise RuntimeError("Failed to create GLFW window")

        # HWND нужен только под Windows.
        self.hwnd = glfw.get_win32_window(self.handle)

        self.width, self.height = width, height
        self.title = title
        self.input = InputManager(self.handle)

        # После инициализации бекенд будет присвоен в Engine.__init__
        self.backend: Optional["alkash3d.graphics.backend.GraphicsBackend"] = None

        glfw.set_framebuffer_size_callback(self.handle, self._on_resize)

        # ---------------------------------------------------------
        #  **Важный момент** – делаем рабочий каталог тем же,
        #  что и каталог, из которого запущен пример.
        #  Это избавляет от необходимости в каждом скрипте делать
        #  `os.chdir(...)`.
        # ---------------------------------------------------------
        script_dir = os.path.abspath(os.path.dirname(__file__))
        os.chdir(os.path.abspath(os.path.join(script_dir, "..")))   # <-- перейти в корень проекта
        # Если вы запускаете пример из любого места, cwd теперь будет
        # корневой каталог репозитория, где находятся `examples`,
        # `resources` и т.д.

        # V‑sync будет включён позже через Engine.set_vsync()
        self.set_vsync(True)

    # ---------------------------------------------------------
    def _on_resize(self, _win, w: int, h: int) -> None:
        self.width, self.height = w, h

    # ---------------------------------------------------------
    def set_vsync(self, enable: bool = True) -> None:
        """
        Делегируем настройку V‑sync бекенду (DX12‑бекенд умеет
        менять sync‑interval у Present).  В GL‑режиме можно было бы
        вызвать glfw.swap_interval().
        """
        self._vsync = enable
        if self.backend is not None:
            self.backend.set_vsync(enable)

    # ---------------------------------------------------------
    def should_close(self) -> bool:
        return glfw.window_should_close(self.handle)

    # ---------------------------------------------------------
    def swap_buffers(self) -> None:
        """
        Для DX12 вызываем present у бекенда.
        (GL‑бэкенд пока не реализован, но здесь могла бы стоять
        glfw.swap_buffers(self.handle)).
        """
        if self.backend is not None:
            self.backend.present()

    # ---------------------------------------------------------
    def poll_events(self) -> None:
        glfw.poll_events()

    # ---------------------------------------------------------
    def close(self) -> None:
        glfw.set_window_should_close(self.handle, True)

    # ---------------------------------------------------------
    def resource_path(self, relative_path: str) -> Path:
        """Возвратить абсолютный путь к ресурсу внутри проекта."""
        repo_root = Path(__file__).resolve().parents[1]
        return repo_root / "resources" / relative_path