# alkash3d/renderer/pipelines/forward.py
# -*- coding: utf-8 -*-

from __future__ import annotations
import os
from typing import Optional
from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger


class ForwardRenderer:
    """Forward rendering pipeline."""

    def __init__(self, window, backend):
        self.window = window
        self.backend = backend
        self.shader: Optional[Shader] = None

        # Пути к шейдерам
        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(__file__))))
        vs_path = os.path.join(base_dir, "resources", "shaders", "forward_vert.hlsl")
        fs_path = os.path.join(base_dir, "resources", "shaders", "forward_frag.hlsl")

        logger.info(f"[ForwardRenderer] Loading shaders: {vs_path}, {fs_path}")

        # Создаём шейдер с правильными именами параметров
        self.shader = Shader(
            backend=self.backend,
            vs_path=vs_path,
            ps_path=fs_path
        )

        if not self.shader.compile():
            raise RuntimeError("Failed to compile shaders")

        logger.info("[ForwardRenderer] Initialized successfully")

    def render(self, scene, camera):
        """Рендерит сцену."""
        if not self.shader or not self.shader.use():
            logger.error("[ForwardRenderer] shader.use() failed")
            return

        # Получаем RTV для текущего кадра
        frame_index = self.backend.get_frame_index()
        if frame_index >= len(self.backend._rtv_cpu_handles):
            logger.error(f"[ForwardRenderer] Invalid frame index: {frame_index}")
            return

        rtv = self.backend._rtv_cpu_handles[frame_index]

        # Устанавливаем render target
        if not self.backend.set_render_target(rtv):
            logger.error("[ForwardRenderer] Failed to set render target")
            return

        # Очищаем экран
        clear_color = (0.2, 0.3, 0.5, 1.0)
        if not self.backend.clear_render_target(rtv, clear_color):
            logger.error("[ForwardRenderer] Failed to clear render target")
            return

        # Устанавливаем viewport и scissor
        self.backend.set_viewport(0, 0, self.window.width, self.window.height)
        self.backend.set_scissor_rect(0, 0, self.window.width, self.window.height)

        # Получаем видимые узлы
        visible_nodes = scene.visible_nodes(camera)

        # Отрисовываем каждый видимый меш
        drawn = 0
        for node in visible_nodes:
            if hasattr(node, 'draw'):
                try:
                    node.draw(self.backend)
                    drawn += 1
                except Exception as e:
                    logger.error(f"[ForwardRenderer] Error drawing {node.name}: {e}")

        logger.debug(f"[ForwardRenderer] Drawn {drawn} nodes")

        # Завершаем кадр
        if not self.backend.end_frame():
            logger.error("[ForwardRenderer] end_frame failed")
            return

        # Презентуем
        sync_interval = 1 if self.backend._vsync_enabled else 0
        if not self.backend.present(sync_interval):
            logger.error("[ForwardRenderer] present failed")

    def resize(self, width: int, height: int):
        """Обрабатывает изменение размера окна."""
        self.window.width = width
        self.window.height = height
        # Шейдер не требует пересоздания при resize
        logger.debug(f"[ForwardRenderer] Resized to {width}x{height}")

    def cleanup(self):
        """Освобождает ресурсы."""
        if self.shader:
            self.shader.cleanup()
            self.shader = None
        logger.info("[ForwardRenderer] Cleaned up")