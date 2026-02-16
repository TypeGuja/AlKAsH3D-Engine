# alkash3d/renderer/pipelines/forward.py
# ------------------------------------------------------------
# Forward‑renderer с корректным созданием белой 1×1‑текстуры
# (UPLOAD‑heap → UpdateTexture) и без «перезаписи» CBV‑слота.
# ------------------------------------------------------------

import numpy as np
from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger
from alkash3d.graphics import select_backend

class ForwardRenderer:
    """
    Простой forward‑pipeline.
    Если у меша нет материала – используется 1×1‑белая placeholder‑текстура.
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

        # ---------- 2️⃣ Белая placeholder ----------
        self._create_white_placeholder()

        # ---------- 3️⃣ Дескриптор‑хип ----------
        if self.backend.cbv_srv_uav_heap:
            self.backend.set_descriptor_heaps([self.backend.cbv_srv_uav_heap])

        # ---------- 4️⃣ PSO ----------
        self.backend.set_graphics_pipeline(self.shader.pso)

    def _create_white_placeholder(self):
        """Создать 1×1‑белую текстуру и SRV."""
        # Белый пиксель – 0xFF 0xFF 0xFF 0xFF
        white_pixel = (255).to_bytes(1, "little") * 4

        # 1) создаём буфер‑загрузку (UPLOAD‑heap)
        upload_buf = self.backend.create_buffer(white_pixel, usage="upload")

        # 2) собственно создаём текстуру (DEFAULT‑heap)
        self.white_tex = self.backend.create_texture(
            data=None,          # никаких начальных данных – сразу будем копировать
            w=1,
            h=1,
            fmt="RGBA8",
        )

        # 3) копируем данные из upload‑буфера в текстуру
        self.backend.update_texture(self.white_tex, white_pixel, w=1, h=1)

        # 4) SRV‑дескриптор
        srv_idx = self.backend.cbv_srv_uav_heap.next_free()
        cpu_handle = self.backend.cbv_srv_uav_heap.get_cpu_handle(srv_idx)
        self.backend.create_shader_resource_view(self.white_tex, cpu_handle)

        # 5) сохраняем GPU‑хендл для быстрых bind‑ов в материалах
        self.default_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(srv_idx)

    # ------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    # ------------------------------------------------------------
    def render(self, scene, camera) -> None:
        self.backend.begin_frame()
        self.backend.set_viewport(0, 0,
                                 self.window.width, self.window.height)
        self.backend.set_scissor_rect(0, 0,
                                      self.window.width, self.window.height)

        view = camera.get_view_matrix()
        proj = camera.get_projection_matrix(self.window.width / self.window.height)

        self.shader.set_uniform_mat4("uView", view)
        self.shader.set_uniform_mat4("uProj", proj)
        self.shader.set_uniform_vec3("uCamPos", camera.position)

        self.shader.use()

        rtv0 = self.backend.rtv_heap.get_cpu_handle(0)
        self.backend.set_render_target(rtv0)
        self.backend.clear_render_target(rtv0, (0.07, 0.07, 0.08, 1.0))

        for node in scene.traverse():
            if not hasattr(node, "draw"):
                continue

            # Если у узла нет собственного материала – привязываем placeholder
            if hasattr(node, "material"):
                node.material.bind(self.backend)
            else:
                # Слот 0 уже содержит CBV, а слот 1 – наш белый SRV.
                self.backend.set_root_descriptor_table(1, self.default_srv_gpu)

            self.shader.set_uniform_mat4("uModel", node.get_world_matrix().to_gl())

            if hasattr(node, "color"):
                self.shader.set_uniform_vec3("uTint", node.color)
            else:
                self.shader.set_uniform_vec3(
                    "uTint", np.array([1.0, 1.0, 1.0], np.float32)
                )

            node.draw(self.backend)

        self.backend.end_frame()