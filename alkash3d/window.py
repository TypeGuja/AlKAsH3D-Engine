"""
Окно + GLFW‑контекст (без OpenGL‑контекста, нужен для DX12).
"""

import glfw
from alkash3d.core.input import InputManager
from pathlib import Path

class Window:
    """Окно + GLFW‑контекст."""
    def __init__(self, width: int = 1280, height: int = 720, title: str = "AlKAsH3D Engine"):
        if not glfw.init():
            raise RuntimeError("Failed to initialize GLFW")

        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)

        self.handle = glfw.create_window(width, height, title, None, None)
        if not self.handle:
            glfw.terminate()
            raise RuntimeError("Failed to create GLFW window")
        self.hwnd = glfw.get_win32_window(self.handle)

        self.width, self.height = width, height
        self.title = title
        self.input = InputManager(self.handle)

        glfw.set_framebuffer_size_callback(self.handle, self._on_resize)
        self.set_vsync(True)

    def _on_resize(self, _win, w, h):
        self.width, self.height = w, h

    def set_vsync(self, enable: bool = True):
        pass

    def should_close(self) -> bool:
        return glfw.window_should_close(self.handle)

    def swap_buffers(self):
        pass

    def poll_events(self):
        glfw.poll_events()

    def close(self):
        glfw.set_window_should_close(self.handle, True)

    def __del__(self):
        try:
            glfw.terminate()
        except Exception:
            pass

    def resource_path(self, relative_path: str) -> Path:
        repo_root = Path(__file__).resolve().parents[1]
        return repo_root / "resources" / relative_path
