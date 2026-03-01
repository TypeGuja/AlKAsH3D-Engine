# alkash3d/renderer/pipelines/forward.py
# -*- coding: utf-8 -*-
"""
Простой forward‑pipeline.
Исправлен синтаксис условия и импорт select_backend.
"""

import numpy as np
import ctypes
from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger
from alkash3d.graphics import select_backend
from alkash3d.graphics.utils import d3d12_wrapper as dx  # for debug prints


class ForwardRenderer:
    """
    Простой forward‑pipeline.
    """

    def __init__(self, window, backend=None):
        self.window = window
        self.backend = backend or select_backend("dx12")
        self.width, self.height = window.width, window.height

        # Формируем абсолютные пути к шейдерам
        vs_path = str(window.resource_path("shaders/forward_vert.hlsl"))
        fs_path = str(window.resource_path("shaders/forward_frag.hlsl"))

        logger.info(f"[ForwardRenderer] Loading shaders: {vs_path}, {fs_path}")

        try:
            self.shader = Shader(
                vertex_path=vs_path,
                fragment_path=fs_path,
                backend=self.backend,
            )
        except Exception as e:
            logger.error(f"[ForwardRenderer] Shader creation failed: {e}")
            self.shader = None

        # Белый 1×1‑placeholder‑текстура (используется, если у материала нет карт)
        self._create_white_placeholder()

        # Устанавливаем descriptor‑heaps один раз (если они существуют)
        if hasattr(self.backend, "rtv_heap") and hasattr(self.backend, "cbv_srv_uav_heap"):
            try:
                self.backend.set_descriptor_heaps([self.backend.rtv_heap, self.backend.cbv_srv_uav_heap])
            except Exception as e:
                logger.error(f"[ForwardRenderer] set_descriptor_heaps error: {e}")

        # Привязываем PSO сразу, если шейдер успешно скомпилировался
        if self.shader and hasattr(self.shader, "pso") and self.shader.pso:
            try:
                self.backend.set_graphics_pipeline(self.shader.pso)
            except Exception as e:
                logger.error(f"[ForwardRenderer] set_graphics_pipeline error: {e}")

    # ------------------------------------------------------------------
    def _create_white_placeholder(self):
        """
        Создаёт 1×1‑белую текстуру и SRV.
        """
        try:
            white_pixel = (255).to_bytes(1, "little") * 4  # RGBA=255,255,255,255

            self.white_tex = self.backend.create_texture(
                data=white_pixel, w=1, h=1, fmt="RGBA8"
            )
            # SRV‑дескриптор
            if self.backend.cbv_srv_uav_heap:
                idx = self.backend.cbv_srv_uav_heap.next_free()
                cpu = self.backend.cbv_srv_uav_heap.get_cpu_handle(idx)
                if self.backend.create_shader_resource_view(self.white_tex, cpu):
                    self.default_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(idx)
                else:
                    self.default_srv_gpu = 0
            else:
                self.default_srv_gpu = 0
        except Exception as e:
            logger.error(f"[ForwardRenderer] white placeholder error: {e}")
            self.default_srv_gpu = 0

    # ------------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    # ------------------------------------------------------------------
    def render(self, scene, camera):
        """Главный цикл отрисовки."""
        try:
            if not self.backend.begin_frame():
                logger.error("[ForwardRenderer] begin_frame failed")
                return

            # ---- Выбираем back‑buffer RTV (если есть) ----
            if (
                hasattr(self.backend, "rtv_heap")
                and self.backend.rtv_heap
                and self.backend.rtv_heap.num_descriptors > 0
            ):
                try:
                    frame_idx = self.backend.get_frame_index() % dx.SWAP_CHAIN_BUFFER_COUNT
                    back_rtv = self.backend.rtv_heap.get_cpu_handle(frame_idx)
                    self.backend.set_render_target(back_rtv)
                    self.backend.clear_render_target(back_rtv, (0.1, 0.1, 0.2, 1.0))
                except Exception as e:
                    logger.error(f"[ForwardRenderer] clear/render_target error: {e}")

            # ---- Шейдер ----
            if self.shader and self.shader.pso:
                if not self.shader.use():
                    logger.warning("[ForwardRenderer] shader.use() returned False")
                    self.backend.end_frame()
                    return

                # ---- Камера ----
                try:
                    aspect = self.window.width / max(self.window.height, 1.0)
                    view = camera.get_view_matrix()
                    proj = camera.get_projection_matrix(aspect)
                    self.shader.set_uniform_mat4("uView", view)
                    self.shader.set_uniform_mat4("uProj", proj)
                except Exception as e:
                    logger.error(f"[ForwardRenderer] camera uniform error: {e}")

                # ---- Рисуем все узлы ----
                rendered = 0
                for node in scene.traverse():
                    if not getattr(node, "draw", None):
                        continue
                    if getattr(node, "visible", True) is False:
                        continue
                    try:
                        model = node.get_world_matrix()
                        self.shader.set_uniform_mat4("uModel", model.to_gl())
                    except Exception as e:
                        logger.error(f"[ForwardRenderer] model matrix error: {e}")
                        continue

                    # Привязываем материал (если есть)
                    if hasattr(node, "material") and node.material:
                        try:
                            node.material.bind(self.backend)
                        except Exception as e:
                            logger.error(f"[ForwardRenderer] material bind error: {e}")
                            # fallback – белый placeholder
                            if self.default_srv_gpu:
                                self.backend.set_root_descriptor_table(1, self.default_srv_gpu)
                    elif self.default_srv_gpu:
                        self.backend.set_root_descriptor_table(1, self.default_srv_gpu)

                    # Рисуем геометрию
                    node.draw(self.backend)
                    rendered += 1
                logger.debug(f"[ForwardRenderer] rendered {rendered} drawables")
            else:
                logger.warning("[ForwardRenderer] No valid shader/PSO – nothing drawn")

            # ---- Завершаем кадр ----
            if not self.backend.end_frame():
                logger.error("[ForwardRenderer] end_frame failed")
        except Exception as e:
            logger.error(f"[ForwardRenderer] render error: {e}")
            import traceback

            traceback.print_exc()
