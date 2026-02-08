# alkash3d/window.py
# ---------------------------------------------------------------
# Оконная подсистема – обёртка над glfw.
# ---------------------------------------------------------------
import glfw
from OpenGL import GL

from alkash3d.core.input import InputManager


class Window:
    """Окно + контекст OpenGL."""
    def __init__(self, width: int = 1280, height: int = 720,
                 title: str = "AlKAsH3D Engine"):
        if not glfw.init():
            raise RuntimeError("Failed to initialize GLFW")
        glfw.window_hint(glfw.CONTEXT_VERSION_MAJOR, 4)
        glfw.window_hint(glfw.CONTEXT_VERSION_MINOR, 5)
        glfw.window_hint(glfw.OPENGL_PROFILE, glfw.OPENGL_CORE_PROFILE)

        self.handle = glfw.create_window(width, height, title, None, None)
        if not self.handle:
            glfw.terminate()
            raise RuntimeError("Failed to create GLFW window")

        glfw.make_context_current(self.handle)

        self.width = width
        self.height = height
        self.title = title

        self.input = InputManager(self.handle)

        # callback for resize
        glfw.set_framebuffer_size_callback(self.handle, self._on_resize)

    def _on_resize(self, window, w, h):
        self.width, self.height = w, h
        GL.glViewport(0, 0, w, h)

    def should_close(self) -> bool:
        return glfw.window_should_close(self.handle)

    def swap_buffers(self):
        glfw.swap_buffers(self.handle)

    def poll_events(self):
        glfw.poll_events()

    def close(self):
        glfw.set_window_should_close(self.handle, True)

    def __del__(self):
        # glfw.terminate() может бросить исключение, если уже был вызван.
        try:
            glfw.terminate()
        except Exception:
            pass