# alkash3d/renderer/pipelines/forward.py
# -*- coding: utf-8 -*-
"""
Forward‑renderer - ИСПРАВЛЕННАЯ ВЕРСИЯ
"""

import numpy as np
from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger
from alkash3d.graphics import select_backend


class ForwardRenderer:
    """
    Простой forward‑pipeline.
    """

    def __init__(self, window, backend=None):
        self.window = window
        self.backend = backend or select_backend("dx12")

        # ---------- 1️⃣ Шейдер ----------
        self.shader = Shader(
            vertex_path=str(window.resource_path("shaders/forward_vert.hlsl")),
            fragment_path=str(window.resource_path("shaders/forward_frag.hlsl")),
            backend=self.backend,
        )

        # ---------- 2️⃣ Белая placeholder (ИСПРАВЛЕНО) ----------
        self._create_white_placeholder()

        # ---------- 3️⃣ Дескриптор‑хип ----------
        self.backend.set_descriptor_heaps([self.backend.rtv_heap, self.backend.cbv_srv_uav_heap])

        # ---------- 4️⃣ PSO ----------
        if hasattr(self.shader, 'pso') and self.shader.pso:
            self.backend.set_graphics_pipeline(self.shader.pso)

    # -----------------------------------------------------------------
    def _create_white_placeholder(self):
        """Создать 1×1‑белую текстуру и SRV."""
        white_pixel = (255).to_bytes(1, "little") * 4  # RGBA = (255,255,255,255)

        # ИСПРАВЛЕНИЕ: width и height вместо w и h
        self.white_tex = self.backend.create_texture(
            data=white_pixel,
            width=1,  # было w=
            height=1,  # было h=
            fmt="RGBA8",
        )

        # SRV‑дескриптор
        if self.backend.cbv_srv_uav_heap:
            srv_idx = self.backend.cbv_srv_uav_heap.next_free()
            cpu_handle = self.backend.cbv_srv_uav_heap.get_cpu_handle(srv_idx)
            self.backend.create_shader_resource_view(self.white_tex, cpu_handle)
            self.default_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(srv_idx)
        else:
            self.default_srv_gpu = 0

    # -----------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    # -----------------------------------------------------------------
    def render(self, scene, camera):
        """Основной цикл рендеринга."""
        try:
            self.backend.begin_frame()

            # Очистка
            if self.backend.rtv_heap and self.backend.rtv_heap.num_descriptors > 0:
                back_rtv = self.backend.rtv_heap.get_cpu_handle(0)
                self.backend.set_render_target(back_rtv)
                self.backend.clear_render_target(back_rtv, (0.1, 0.1, 0.2, 1.0))

            # Используем шейдер
            if hasattr(self, 'shader'):
                self.shader.use()

                # Матрицы камеры
                view_mat = camera.get_view_matrix()
                proj_mat = camera.get_projection_matrix(self.window.width / self.window.height)

                self.shader.set_uniform_mat4("uView", view_mat)
                self.shader.set_uniform_mat4("uProj", proj_mat)

                # Рендерим все объекты
                for node in scene.traverse():
                    if hasattr(node, "draw") and getattr(node, "visible", True):
                        # Model matrix
                        model_mat = node.get_world_matrix()
                        if hasattr(model_mat, 'to_gl'):
                            model_mat = model_mat.to_gl()
                        self.shader.set_uniform_mat4("uModel", model_mat)

                        # Материал
                        if hasattr(node, 'material') and node.material:
                            node.material.bind(self.backend)
                        elif hasattr(self, 'default_srv_gpu') and self.default_srv_gpu:
                            self.backend.set_root_descriptor_table(1, self.default_srv_gpu)

                        # Отрисовка
                        node.draw(self.backend)

                # Отправляем uniform-ы
                if hasattr(self.shader, 'flush'):
                    self.shader.flush()

            self.backend.end_frame()

        except Exception as e:
            logger.error(f"[ForwardRenderer] Render error: {e}")