# alkash3d/engine.py
# -*- coding: utf-8 -*-
"""
Главный цикл движка.

* Инициализирует окно, DX12‑бэкенд и нужный рендерер.
* После создания бэкенда увеличивает размер RTV‑heap и сразу
  пересоздаёт RTV‑дескрипторы (иначе получаем чёрный кадр).
* Включён V‑Sync, FPS‑counter и система плагинов.
"""
import time
import glfw
from alkash3d.core.timer import Timer
from alkash3d.scene import Scene, Camera
from alkash3d.utils import logger, Config, FPSCounter, Profiler
from alkash3d.utils.logger import gl_check_error
from alkash3d.postproc import (
    PostProcessingPipeline,
    BloomPass,
    SSAOPass,
    TemporalAAPass,
    ColorGradingPass,
    TonemapPass,
)
from alkash3d.plugins import PluginManager
from alkash3d.renderer.pipelines.forward import ForwardRenderer
from alkash3d.renderer.pipelines.deferred import DeferredRenderer
from alkash3d.renderer.pipelines.hybrid import HybridRenderer
from alkash3d.renderer.pipelines.rtx_renderer import RTXRenderer
from alkash3d.graphics import select_backend
from alkash3d.graphics.gl_backend import GLBackend   # только для тип‑чеков
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap   # NEW


class Engine:
    """
    Главный цикл движка.
    """
    # -----------------------------------------------------------------
    def __init__(
        self,
        width: int = 1280,
        height: int = 720,
        title: str = "AlKAsH3D Engine",
        renderer: str = "forward",          # forward | deferred | hybrid | rtx
        backend_name: str = "dx12",        # dx12 | gl
    ):
        # ---------------------------------------------------------
        # 0️⃣  Конфиг + окно
        # ---------------------------------------------------------
        self.cfg = Config()
        win_cfg = self.cfg["window"]
        self.window = self._create_window(
            win_cfg.get("width", width),
            win_cfg.get("height", height),
            win_cfg.get("title", title),
        )

        # ---------------------------------------------------------
        # 1️⃣  Выбор и инициализация графического бекенда
        # ---------------------------------------------------------
        self.backend = select_backend(backend_name)
        self.backend.init_device(
            self.window.hwnd,
            self.window.width,
            self.window.height,
        )
        self.backend.set_viewport(0, 0,
                                 self.window.width, self.window.height)
        self.backend.set_scissor_rect(0, 0,
                                      self.window.width, self.window.height)

        # ---------------------------------------------------------
        # 2️⃣  Увеличиваем RTV‑heap (по‑умолчанию 2 дескриптора) и
        #     сразу пересоздаём RTV‑дескрипторы для swap‑chain.
        # ---------------------------------------------------------
        # +1 «свободный» дескриптор (нужен, если захотим ещё какой‑нибудь RTV)
        new_rtv_cnt = self.backend.rtv_heap.num_descriptors + 1
        self.backend.rtv_heap = DescriptorHeap(
            device=self.backend.device,
            num_descriptors=new_rtv_cnt,
            heap_type="rtv",
        )
        # После замены heap создаём RTV‑дескрипторы заново
        self.backend.recreate_swapchain_rtv()

        # ---------------------------------------------------------
        # 3️⃣  Сцена + камера
        # ---------------------------------------------------------
        self.scene = Scene()
        self.camera = Camera()
        self.scene.add_child(self.camera)

        # ---------------------------------------------------------
        # 4️⃣  Выбор рендера
        # ---------------------------------------------------------
        if renderer == "forward":
            self.renderer = ForwardRenderer(self.window, self.backend)
        elif renderer == "deferred":
            self.renderer = DeferredRenderer(self.window, self.backend)
        elif renderer == "hybrid":
            self.renderer = HybridRenderer(self.window, self.backend)
        elif renderer == "rtx":
            self.renderer = RTXRenderer(self.window, self.backend)
        else:
            raise ValueError(f"Unknown renderer mode: {renderer}")

        # ---------------------------------------------------------
        # 5️⃣  Пост‑процессинг (только для GL‑бэкенда)
        # ---------------------------------------------------------
        if isinstance(self.backend, GLBackend):
            self.postprocess = PostProcessingPipeline(
                self.window.width, self.window.height
            )
            self.postprocess.add_pass(BloomPass())
            self.postprocess.add_pass(SSAOPass())
            self.postprocess.add_pass(TemporalAAPass())
            self.postprocess.add_pass(ColorGradingPass())
            self.postprocess.add_pass(TonemapPass())
        else:
            self.postprocess = None

        # ---------------------------------------------------------
        # 6️⃣  Плагины
        # ---------------------------------------------------------
        self.plugin_manager = PluginManager()
        self.plugin_manager.discover()

        # Добавляем плагины только если есть post‑process pipeline
        if self.postprocess:
            for name, cls in self.plugin_manager.passes.items():
                self.postprocess.add_pass(cls())

        # Если у рендера есть атрибут postproc – связываем их
        if hasattr(self.renderer, "postproc"):
            self.renderer.postproc = self.postprocess

        # ---------------------------------------------------------
        # 7️⃣  V‑Sync, таймер, FPS‑counter
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

            # F9 – FPS‑display, F10 – V‑Sync
            self._handle_toggle_key(glfw.KEY_F9, "show_fps", "FPS display")
            self._handle_toggle_key(glfw.KEY_F10, "v_sync", "V‑Sync")

            if self._editor:
                self._editor.update(dt)

            self.scene.update(dt)

            # Render + (если у рендера нет собственного post‑proc)
            self.renderer.render(self.scene, self.camera)

            if not hasattr(self.renderer, "postproc") and self.postprocess:
                self.postprocess.run(self.backend)

            self.window.swap_buffers()

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
