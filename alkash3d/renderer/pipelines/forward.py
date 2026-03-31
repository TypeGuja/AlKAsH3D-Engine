# alkash3d/renderer/pipelines/forward.py
# -*- coding: utf-8 -*-

from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger
from alkash3d.graphics import select_backend
from alkash3d.graphics.utils import d3d12_wrapper as dx


class ForwardRenderer:
    """Простой forward‑pipeline."""

    def __init__(self, window, backend=None):
        self.window = window
        self.backend = backend or select_backend("dx12")
        self.width, self.height = window.width, window.height

        # Шейдеры
        vs_path = str(window.resource_path("shaders/forward_vert.hlsl"))
        fs_path = str(window.resource_path("shaders/forward_frag.hlsl"))

        logger.info(f"[ForwardRenderer] Loading shaders: {vs_path}, {fs_path}")

        self.shader = Shader(
            vertex_path=vs_path,
            fragment_path=fs_path,
            backend=self.backend,
        )

        # Создаём белую текстуру
        self._create_white_placeholder()

        # Флаги
        self._heap_set = False

    # ------------------------------------------------------------------
    def _create_white_placeholder(self):
        """Создаёт 1×1‑белую текстуру и SRV в слоте TEXTURE"""
        try:
            logger.info("[ForwardRenderer] Creating white placeholder texture")

            white_pixel = b'\xff\xff\xff\xff'

            # Создаём текстуру
            self.white_tex = self.backend.create_texture(
                data=white_pixel,
                w=1,
                h=1,
                fmt="RGBA8"
            )

            if not self.white_tex or not self.white_tex.ptr:
                raise RuntimeError("Failed to create white texture")

            # Создаём SRV для текстуры в слоте TEXTURE
            tex_cpu = self.backend.cbv_srv_uav_heap.get_cpu_handle(self.shader.tex_slot)

            if not self.backend.create_shader_resource_view(self.white_tex, tex_cpu):
                raise RuntimeError("Failed to create SRV for white texture")

            logger.info(f"[ForwardRenderer] White texture SRV created at slot {self.shader.tex_slot}")

        except Exception as e:
            logger.error(f"[ForwardRenderer] white placeholder error: {e}")
            raise

    # ------------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        logger.debug(f"[ForwardRenderer] resize({w}, {h})")
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    # ------------------------------------------------------------------
    def render(self, scene, camera):
        try:
            # 1. Устанавливаем дескрипторные хипы (один раз)
            if not self._heap_set and hasattr(self.backend, "cbv_srv_uav_heap") and self.backend.cbv_srv_uav_heap:
                heap = self.backend.cbv_srv_uav_heap
                heap_ptr = heap.heap_ptr
                if heap_ptr and heap_ptr.value:
                    logger.info(f"[ForwardRenderer] Setting descriptor heap once: 0x{heap_ptr.value:X}")
                    self.backend.set_descriptor_heaps([heap_ptr])
                    self._heap_set = True

            # 2. Начинаем кадр
            if not self.backend.begin_frame():
                logger.error("[ForwardRenderer] begin_frame failed")
                return

            # 3. Устанавливаем render target
            if (hasattr(self.backend, "rtv_heap") and self.backend.rtv_heap
                    and self.backend.rtv_heap.num_descriptors > 0):
                frame_idx = self.backend.get_frame_index() % dx.SWAP_CHAIN_BUFFER_COUNT
                back_rtv = self.backend.rtv_heap.get_cpu_handle(frame_idx)
                self.backend.set_render_target(back_rtv)
                self.backend.clear_render_target(back_rtv, (0.2, 0.3, 0.4, 1.0))

            # 4. Устанавливаем PSO и descriptor table
            if not self.shader.use():
                logger.error("[ForwardRenderer] shader.use() failed")
                self.backend.end_frame()
                return

            # 5. Обновляем uniform'ы
            aspect = self.window.width / max(self.window.height, 1.0)
            view = camera.get_view_matrix()
            proj = camera.get_projection_matrix(aspect)
            self.shader.set_uniform_mat4("uView", view)
            self.shader.set_uniform_mat4("uProj", proj)
            self.shader.set_uniform_vec4("uTint", (1.0, 1.0, 1.0, 1.0))

            # 6. Отрисовка всех узлов
            for node in scene.traverse():
                if not hasattr(node, "draw"):
                    continue
                if hasattr(node, "visible") and node.visible is False:
                    continue

                model = node.get_world_matrix()
                self.shader.set_uniform_mat4("uModel", model.to_gl())

                # Только обновляем буфер
                self.shader.flush()

                node.draw(self.backend)

            # 7. Завершаем кадр
            if not self.backend.end_frame():
                logger.error("[ForwardRenderer] end_frame failed")
                return

            # 8. Сбрасываем флаг для следующего кадра
            self.shader.reset_descriptor_table_flag()

            # 9. Отображаем кадр
            if hasattr(self.backend, 'present'):
                self.backend.present()

        except Exception as e:
            logger.error(f"[ForwardRenderer] render error: {e}")
            import traceback
            traceback.print_exc()