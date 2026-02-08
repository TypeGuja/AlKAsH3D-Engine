# alkas3d/engine.py
# ---------------------------------------------------------------
# Главный цикл движка. Поддерживает три режима:
#   - forward (по‑умолчанию)
#   - deferred
#   - rt (CUDA‑трассировка)
# ---------------------------------------------------------------

import time
import glfw                                                  # ← необходим для колбэка

from alkash3d.renderer import ForwardRenderer, DeferredRenderer, RTPipeline
from alkash3d.scene import Scene
from alkash3d.utils import logger


class Engine:
    """Класс‑обёртка над окном, сценой, камерой и выбранным пайплайном."""
    def __init__(self,
                 width: int = 1280,
                 height: int = 720,
                 title: str = "AlKAsH3D Engine",
                 renderer: str = "forward"):
        # ---- Окно -------------------------------------------------
        from alkash3d import Window
        self.window = Window(width, height, title)

        # ---- Сцена + камера ------------------------------------
        self.scene = Scene()
        from alkash3d import Camera
        self.camera = Camera()
        self.scene.add_child(self.camera)

        # ---- Выбор рендер‑потока -------------------------------
        if renderer == "forward":
            self.renderer = ForwardRenderer(self.window)
        elif renderer == "deferred":
            self.renderer = DeferredRenderer(self.window)
        elif renderer == "rt":
            self.renderer = RTPipeline(self.window)
        else:
            raise ValueError(f"Unknown renderer mode: {renderer}")

        # ---- Resize‑callback ------------------------------------
        glfw.set_framebuffer_size_callback(
            self.window.handle,
            lambda win, w, h: self.renderer.resize(w, h)
        )

        self.last_time = time.time()

    # -----------------------------------------------------------------
    # Основной цикл
    # -----------------------------------------------------------------
    def run(self):
        from alkash3d.utils import logger
        logger.info("Engine started")
        while not self.window.should_close():
            now = time.time()
            dt = now - self.last_time
            self.last_time = now

            # ---- INPUT -------------------------------------------------
            self.window.poll_events()
            self.camera.update_fly(dt, self.window.input)

            # ---- UPDATE ------------------------------------------------
            self.scene.update(dt)

            # ---- RENDER ------------------------------------------------
            self.renderer.render(self.scene, self.camera)
            self.window.swap_buffers()

        self.shutdown()

    def shutdown(self):
        logger.info("Engine shutting down")
        self.window.close()