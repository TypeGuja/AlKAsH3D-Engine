# alkash3d/renderer/pipelines/forward.py
# -*- coding: utf-8 -*-
"""
Простой forward‑pipeline.
ИСПРАВЛЕННАЯ ВЕРСИЯ с корректной обработкой текстур и отладкой
"""

import numpy as np
import ctypes
from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger
from alkash3d.graphics import select_backend
from alkash3d.graphics.utils import d3d12_wrapper as dx


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
            logger.info("[ForwardRenderer] Shader created successfully")
        except Exception as e:
            logger.error(f"[ForwardRenderer] Shader creation failed: {e}")
            import traceback
            traceback.print_exc()
            self.shader = None

        # Белый 1×1‑placeholder‑текстура (используется, если у материала нет карт)
        self._create_white_placeholder()

        # В D3D12 через SetDescriptorHeaps можно устанавливать только shader‑visible heaps
        if hasattr(self.backend, "cbv_srv_uav_heap"):
            try:
                if self.backend.cbv_srv_uav_heap:
                    logger.debug("[ForwardRenderer] Setting descriptor heaps")
                    self.backend.set_descriptor_heaps([self.backend.cbv_srv_uav_heap])
            except Exception as e:
                logger.error(f"[ForwardRenderer] set_descriptor_heaps error: {e}")

        # Привязываем PSO сразу, если шейдер успешно скомпилировался
        if self.shader and hasattr(self.shader, "pso") and self.shader.pso:
            try:
                logger.debug("[ForwardRenderer] Setting graphics pipeline")
                self.backend.set_graphics_pipeline(self.shader.pso)
            except Exception as e:
                logger.error(f"[ForwardRenderer] set_graphics_pipeline error: {e}")

    # ------------------------------------------------------------------
    def _create_white_placeholder(self):
        """
        Создаёт 1×1‑белую текстуру и SRV.
        """
        try:
            logger.info("[ForwardRenderer] Creating white placeholder texture")

            # Создаем 1x1 белый пиксель как bytes (RGBA)
            white_pixel = b'\xff\xff\xff\xff'  # RGBA белый (255,255,255,255)

            # Создаем текстуру
            self.white_tex = self.backend.create_texture(
                data=white_pixel,
                w=1,
                h=1,
                fmt="RGBA8"
            )

            # Если текстура создалась успешно
            if self.white_tex and hasattr(self.white_tex, 'ptr') and self.white_tex.ptr:
                ptr_val = self.white_tex.ptr.value if hasattr(self.white_tex.ptr, 'value') else int(self.white_tex.ptr)
                logger.info(f"[ForwardRenderer] White texture created: {hex(ptr_val)}")

                # SRV‑дескриптор
                if hasattr(self.backend, "cbv_srv_uav_heap") and self.backend.cbv_srv_uav_heap:
                    try:
                        idx = self.backend.cbv_srv_uav_heap.next_free()
                        cpu = self.backend.cbv_srv_uav_heap.get_cpu_handle(idx)

                        if self.backend.create_shader_resource_view(self.white_tex, cpu):
                            self.default_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(idx)
                            logger.info(f"[ForwardRenderer] White SRV created at GPU handle 0x{self.default_srv_gpu:X}")
                        else:
                            logger.error("[ForwardRenderer] Failed to create SRV for white texture")
                            self.default_srv_gpu = 0
                    except Exception as e:
                        logger.error(f"[ForwardRenderer] SRV creation error: {e}")
                        import traceback
                        traceback.print_exc()
                        self.default_srv_gpu = 0
                else:
                    logger.warning("[ForwardRenderer] No CBV/SRV/UAV heap available")
                    self.default_srv_gpu = 0
            else:
                logger.error("[ForwardRenderer] Failed to create white texture")
                self.default_srv_gpu = 0

        except Exception as e:
            logger.error(f"[ForwardRenderer] white placeholder error: {e}")
            import traceback
            traceback.print_exc()
            self.default_srv_gpu = 0

    # ------------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        logger.debug(f"[ForwardRenderer] resize({w}, {h})")
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    # ------------------------------------------------------------------
    def render(self, scene, camera):
        """Главный цикл отрисовки."""
        try:
            logger.debug("[ForwardRenderer] render() called")

            if not self.backend.begin_frame():
                logger.error("[ForwardRenderer] begin_frame failed")
                return

            # ---- Выбираем back‑buffer RTV (если есть) ----
            if (hasattr(self.backend, "rtv_heap")
                    and self.backend.rtv_heap
                    and self.backend.rtv_heap.num_descriptors > 0):
                try:
                    frame_idx = self.backend.get_frame_index() % dx.SWAP_CHAIN_BUFFER_COUNT
                    back_rtv = self.backend.rtv_heap.get_cpu_handle(frame_idx)
                    logger.debug(f"[ForwardRenderer] Setting RTV at frame {frame_idx}, handle 0x{back_rtv:X}")
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
                    logger.debug("[ForwardRenderer] Camera uniforms set")
                except Exception as e:
                    logger.error(f"[ForwardRenderer] camera uniform error: {e}")

                # ---- Рисуем все узлы ----
                rendered = 0
                nodes_list = list(scene.traverse())
                logger.info(f"[ForwardRenderer] Found {len(nodes_list)} nodes in scene")

                for node in nodes_list:
                    if not hasattr(node, "draw"):
                        logger.debug(f"[ForwardRenderer] Node {node.name} has no draw method")
                        continue
                    if hasattr(node, "visible") and node.visible is False:
                        logger.debug(f"[ForwardRenderer] Node {node.name} not visible")
                        continue

                    logger.debug(f"[ForwardRenderer] Drawing node: {node.name}")

                    try:
                        model = node.get_world_matrix()
                        self.shader.set_uniform_mat4("uModel", model.to_gl())
                    except Exception as e:
                        logger.error(f"[ForwardRenderer] model matrix error for {node.name}: {e}")
                        continue

                    # Привязываем материал (если есть)
                    if hasattr(node, "material") and node.material:
                        try:
                            logger.debug(f"[ForwardRenderer] Binding material for {node.name}")
                            node.material.bind(self.backend)
                        except Exception as e:
                            logger.error(f"[ForwardRenderer] material bind error for {node.name}: {e}")
                            # fallback – белый placeholder
                            if self.default_srv_gpu:
                                logger.debug(
                                    f"[ForwardRenderer] Using fallback texture, handle 0x{self.default_srv_gpu:X}")
                                self.backend.set_root_descriptor_table(1, self.default_srv_gpu)
                    elif self.default_srv_gpu:
                        logger.debug(f"[ForwardRenderer] No material, using fallback texture")
                        self.backend.set_root_descriptor_table(1, self.default_srv_gpu)

                    # Рисуем геометрию
                    node.draw(self.backend)
                    rendered += 1

                logger.info(f"[ForwardRenderer] Rendered {rendered} drawable nodes")
            else:
                logger.warning("[ForwardRenderer] No valid shader/PSO – nothing drawn")

            # ---- Завершаем кадр ----
            if not self.backend.end_frame():
                logger.error("[ForwardRenderer] end_frame failed")
        except Exception as e:
            logger.error(f"[ForwardRenderer] render error: {e}")
            import traceback
            traceback.print_exc()