# alkash3d/engine.py
# -*- coding: utf-8 -*-
"""
Главный цикл движка.
"""

import time
import glfw
from alkash3d.utils.timer import Timer
from alkash3d.scene import Scene, Camera
from alkash3d.utils import logger, Config, FPSCounter
from alkash3d.plugins import PluginManager
from alkash3d.renderer.pipelines.forward import ForwardRenderer
from alkash3d.renderer.pipelines.deferred import DeferredRenderer
from alkash3d.renderer.pipelines.hybrid import HybridRenderer
from alkash3d.renderer.pipelines.rtx_renderer import RTXRenderer
from alkash3d.graphics import select_backend


class Engine:
    # -------------------------------------------------------------
    def __init__(self, width: int = 1280, height: int = 720,
                 title: str = "AlKAsH3D Engine",
                 renderer: str = "forward",
                 backend_name: str = "dx12"):
        # 0️⃣ Конфиг + окно
        self.cfg = Config()
        win_cfg = self.cfg["window"]
        self.window = self._create_window(
            win_cfg.get("width", width),
            win_cfg.get("height", height),
            win_cfg.get("title", title),
        )

        # 1️⃣ Выбор и инициализация графического бэкенда
        self.backend = select_backend(backend_name)

        try:
            self.backend.init_device(self.window.hwnd,
                                     self.window.width,
                                     self.window.height)
        except Exception as e:
            logger.error(f"[Engine] Failed to initialize backend: {e}")
            raise

        # привязываем бекенд к окну
        self.window.backend = self.backend

        self.backend.set_viewport(0, 0, self.window.width, self.window.height)
        self.backend.set_scissor_rect(0, 0, self.window.width, self.window.height)

        # 2️⃣ Создаем RTV для back buffer если есть swap chain
        if self.backend.swap_chain and self.backend.swap_chain.value:
            self.backend._create_swapchain_rtv()

        # ---------------------------------------------------------
        # 3️⃣ Сцена + камера
        # ---------------------------------------------------------
        self.scene = Scene()
        self.camera = Camera()
        self.scene.add_child(self.camera)

        # ---------------------------------------------------------
        # 4️⃣ Выбор рендера
        # ---------------------------------------------------------
        try:
            if renderer == "forward":
                from alkash3d.renderer.pipelines.forward import ForwardRenderer
                self.renderer = ForwardRenderer(self.window, self.backend)
            elif renderer == "deferred":
                from alkash3d.renderer.pipelines.deferred import DeferredRenderer
                self.renderer = DeferredRenderer(self.window, self.backend)
            elif renderer == "hybrid":
                from alkash3d.renderer.pipelines.hybrid import HybridRenderer
                self.renderer = HybridRenderer(self.window, self.backend)
            elif renderer == "rtx":
                from alkash3d.renderer.pipelines.rtx_renderer import RTXRenderer
                self.renderer = RTXRenderer(self.window, self.backend)
            else:
                raise ValueError(f"Unknown renderer mode: {renderer}")
        except Exception as e:
            logger.error(f"[Engine] Failed to create renderer: {e}")
            raise

        # ---------------------------------------------------------
        # 5️⃣ Пост‑процессинг
        # ---------------------------------------------------------
        self.postprocess = None

        # ---------------------------------------------------------
        # 6️⃣ Плагины
        # ---------------------------------------------------------
        self.plugin_manager = PluginManager()
        self.plugin_manager.discover()

        # ---------------------------------------------------------
        # 7️⃣ V‑Sync, таймер, FPS‑counter
        # ---------------------------------------------------------
        glfw.set_framebuffer_size_callback(
            self.window.handle,
            lambda win, w, h: self._on_resize(w, h),
        )
        self.set_vsync(bool(self.cfg.get("v_sync", True)))

        self.timer = Timer()
        self.fps_counter = FPSCounter()
        self._last_fps_print = time.time()
        self.show_fps = bool(self.cfg.get("show_fps", True))
        self._key_state = {}
        self._editor = None

        logger.info(f"[Engine] Initialised with {renderer} renderer, {backend_name} backend")

    # -----------------------------------------------------------------
    def _create_window(self, w: int, h: int, title: str):
        from alkash3d.window import Window
        return Window(w, h, title)

    # -----------------------------------------------------------------
    def _on_resize(self, w: int, h: int):
        self.window.width, self.window.height = w, h
        self.backend.resize(w, h)
        self.renderer.resize(w, h)
        if self.postprocess:
            self.postprocess.resize(w, h)

    # -----------------------------------------------------------------
    def set_vsync(self, enable: bool = True):
        """Переключить V‑Sync и сохранить настройку в конфиге."""
        self.window.set_vsync(enable)
        self.cfg["v_sync"] = enable
        logger.info(f"[Engine] V‑Sync {'ON' if enable else 'OFF'}")

    # -----------------------------------------------------------------
    def run(self):
        """Главный игровой цикл."""
        logger.info("[Engine] Engine started")
        while not self.window.should_close():
            dt = self.timer.tick()
            self.window.poll_events()
            self.camera.update_fly(dt, self.window.input)

            # F9 — FPS‑display, F10 — V‑Sync
            self._handle_toggle_key(glfw.KEY_F9, "show_fps", "FPS display")
            self._handle_toggle_key(glfw.KEY_F10, "v_sync", "V‑Sync")

            if self._editor:
                self._editor.update(dt)

            self.scene.update(dt)

            # Рендеринг
            try:
                self.renderer.render(self.scene, self.camera)
            except Exception as e:
                logger.error(f"[Engine] Render error: {e}")
                import traceback
                traceback.print_exc()

            # Пост-процессинг (если есть)
            if self.postprocess:
                self.postprocess.run(self.backend)

            if self.show_fps:
                now = time.time()
                if now - self._last_fps_print >= 1.0:
                    logger.info(f"[Engine] FPS: {self.timer.fps:.2f}")
                    self._last_fps_print = now

        self.shutdown()

    # -----------------------------------------------------------------
    def _handle_toggle_key(self, glfw_key, cfg_name, description):
        im = self.window.input
        pressed = im.is_key_pressed(glfw_key)
        prev = self._key_state.get(glfw_key, False)

        if pressed and not prev:
            cur = bool(self.cfg.get(cfg_name, False))
            self.cfg[cfg_name] = not cur

            if cfg_name == "v_sync":
                self.set_vsync(not cur)
            elif cfg_name == "show_fps":
                self.show_fps = not cur

            logger.info(
                f"[Engine] {description} {'ON' if not cur else 'OFF'}"
            )
        self._key_state[glfw_key] = pressed

    # -----------------------------------------------------------------
    def shutdown(self):
        """Освободить ресурсы и закрыть окно."""
        logger.info("[Engine] Shutting down")
        self.window.close()

        if self.postprocess:
            self.postprocess.cleanup(self.backend)

        if hasattr(self.renderer, "cleanup"):
            self.renderer.cleanup()

        if hasattr(self.backend, "shutdown"):
            self.backend.shutdown()