# alkash3d/renderer/pipelines/forward.py - ИСПРАВЛЕННАЯ ВЕРСИЯ

import numpy as np
import ctypes
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
            logger.error(f"[ForwardRenderer] Failed to create shader: {e}")
            self.shader = None

        self._create_white_placeholder()

        if hasattr(self.backend, 'rtv_heap') and hasattr(self.backend, 'cbv_srv_uav_heap'):
            try:
                self.backend.set_descriptor_heaps([
                    self.backend.rtv_heap,
                    self.backend.cbv_srv_uav_heap
                ])
            except Exception as e:
                logger.error(f"[ForwardRenderer] Failed to set descriptor heaps: {e}")

        if self.shader and hasattr(self.shader, 'pso') and self.shader.pso:
            try:
                self.backend.set_graphics_pipeline(self.shader.pso)
            except Exception as e:
                logger.error(f"[ForwardRenderer] Failed to set PSO: {e}")

    def _create_white_placeholder(self):
        """
        Создаёт 1×1‑белую текстуру и SRV.
        Текстура сразу заполняется данными, поэтому
        вызов update_texture не нужен (он падал на default‑heap).
        """
        try:
            # 4 байта = RGBA(255,255,255,255)
            white_pixel = (255).to_bytes(1, "little") * 4

            # Передаём данные сразу – backend создаст upload‑heap
            self.white_tex = self.backend.create_texture(
                data=white_pixel,
                width=1,
                height=1,
                fmt="RGBA8",
            )

            # Создаём SRV в heap‑а
            if hasattr(self.backend, 'cbv_srv_uav_heap') and self.backend.cbv_srv_uav_heap:
                srv_idx = self.backend.cbv_srv_uav_heap.next_free()
                cpu_handle = self.backend.cbv_srv_uav_heap.get_cpu_handle(srv_idx)
                if self.backend.create_shader_resource_view(self.white_tex, cpu_handle):
                    self.default_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(srv_idx)
                else:
                    self.default_srv_gpu = 0
            else:
                self.default_srv_gpu = 0
        except Exception as e:
            logger.error(f"[ForwardRenderer] Failed to create white placeholder: {e}")
            self.default_srv_gpu = 0

    def resize(self, w: int, h: int) -> None:
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    def render(self, scene, camera):
        """Основной цикл рендеринга."""
        try:
            if not self.backend.begin_frame():
                logger.error("[ForwardRenderer] begin_frame failed")
                return

            if (hasattr(self.backend, 'rtv_heap') and self.backend.rtv_heap and
                    self.backend.rtv_heap.num_descriptors > 0):
                try:
                    frame_idx = self.backend.get_frame_index() % 2
                    back_rtv = self.backend.rtv_heap.get_cpu_handle(frame_idx)
                    self.backend.set_render_target(back_rtv)
                    self.backend.clear_render_target(back_rtv, (0.1, 0.1, 0.2, 1.0))
                except Exception as e:
                    logger.error(f"[ForwardRenderer] Clear failed: {e}")

            if self.shader and hasattr(self.shader, 'pso') and self.shader.pso and self.shader.pso != 0x87654321:
                if not self.shader.use():
                    logger.warning("[ForwardRenderer] use() returned False")
                    self.backend.end_frame()
                    return

                try:
                    aspect = self.window.width / max(self.window.height, 1)
                    view_mat = camera.get_view_matrix()
                    proj_mat = camera.get_projection_matrix(aspect)

                    self.shader.set_uniform_mat4("uView", view_mat)
                    self.shader.set_uniform_mat4("uProj", proj_mat)
                except Exception as e:
                    logger.error(f"[ForwardRenderer] Camera matrices error: {e}")

                rendered_count = 0
                for node in scene.traverse():
                    if hasattr(node, "draw") and getattr(node, "visible", True):
                        try:
                            model_mat = node.get_world_matrix()
                            if hasattr(model_mat, 'to_gl'):
                                model_mat = model_mat.to_gl()
                            self.shader.set_uniform_mat4("uModel", model_mat)

                            if hasattr(node, 'material') and node.material:
                                try:
                                    node.material.bind(self.backend)
                                except Exception as e:
                                    logger.error(f"[ForwardRenderer] Material bind error: {e}")
                                    if hasattr(self, 'default_srv_gpu') and self.default_srv_gpu:
                                        self.backend.set_root_descriptor_table(1, self.default_srv_gpu)
                            elif hasattr(self, 'default_srv_gpu') and self.default_srv_gpu:
                                self.backend.set_root_descriptor_table(1, self.default_srv_gpu)

                            node.draw(self.backend)
                            rendered_count += 1

                        except Exception as e:
                            logger.error(
                                f"[ForwardRenderer] Error drawing node {getattr(node, 'name', 'unknown')}: {e}")

                if rendered_count > 0:
                    debug_print(f"[ForwardRenderer] Rendered {rendered_count} objects")

                if hasattr(self.shader, 'flush'):
                    self.shader.flush()

            if not self.backend.end_frame():
                logger.error("[ForwardRenderer] end_frame failed")

        except Exception as e:
            logger.error(f"[ForwardRenderer] Render error: {e}")
            import traceback
            traceback.print_exc()