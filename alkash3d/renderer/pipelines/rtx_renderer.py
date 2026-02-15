# alkash3d/renderer/pipelines/rtx_renderer.py
from __future__ import annotations
import ctypes
from pathlib import Path
import numpy as np
import json
from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.utils import logger, gl_check_error
from alkash3d.renderer.shader import Shader
from alkash3d.graphics import select_backend
import alkash3d_rtx  # уже скомпилированный Rust‑модуль


# -------------------------------------------------------------
# Корневой каталог проекта → resources/shaders
# -------------------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parents[3]   # …/AlKAsH3D-Engine
SHADER_DIR = PROJECT_ROOT / "resources" / "shaders"


class RTXRenderer(BaseRenderer):
    """
    RTX‑pipeline – мост к чистому Rust‑модулю ``alkash3d_rtx``.
    Делает сериализацию JSON → Rust‑трассировку → вывод как fullscreen‑quad.
    """

    def __init__(self, window, backend=None):
        self.window = window
        self.backend = backend or select_backend("dx12")
        self.width, self.height = window.width, window.height

        # Шейдер, который просто копирует RGBA‑текстуру в кадр.
        self.quad_shader = Shader(
            vertex_path=str(SHADER_DIR / "quad_vert.hlsl"),
            fragment_path=str(SHADER_DIR / "quad_frag.hlsl"),
            backend=self.backend,
        )
        self._setup_quad()
        self.backend.enable_depth_test(False)

        # Дескриптор‑слот для RTX‑текстуры будет создан при первой отрисовке
        self._rtx_srv_gpu = None

    # -----------------------------------------------------------------
    def _setup_quad(self):
        verts = np.array(
            [
                -1.0, -1.0,
                 3.0, -1.0,
                -1.0,  3.0,
            ],
            dtype=np.float32,
        )
        self.quad_vb = self.backend.create_buffer(verts.tobytes(), usage="vertex")

    # -----------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.width, self.height = w, h
        self.backend.set_viewport(0, 0, w, h)

    # -----------------------------------------------------------------
    def _scene_to_payload(self, scene, camera) -> str:
        """Минимальная сериализация сцены → JSON."""
        meshes = []
        for node in scene.traverse():
            if isinstance(node, __import__("alkash3d").Mesh):
                meshes.append(
                    {
                        "vertices": node.vertices.flatten().tolist(),
                        "indices": node.indices.tolist()
                        if node.indices is not None
                        else [],
                        "color": node.color.as_np().tolist(),
                    }
                )
        payload = {
            "meshes": meshes,
            "camera": {
                "position": camera.position.as_np().tolist(),
                "view": camera.get_view_matrix().tolist(),
                "proj": camera.get_projection_matrix(self.width / self.height).tolist(),
            },
        }
        return json.dumps(payload)

    # -----------------------------------------------------------------
    def render(self, scene, camera):
        # 1️⃣ Serialize scene
        scene_json = self._scene_to_payload(scene, camera)

        # 2️⃣ Call Rust module → get RGBA bytes
        try:
            rgba_bytes = alkash3d_rtx.render_frame(scene_json, self.width, self.height)
        except Exception as e:
            logger.error(f"[RTXRenderer] Rust render error: {e}")
            return

        # 3️⃣ Create / update DX12 texture
        if not hasattr(self, "tex"):
            self.tex = self.backend.create_texture(
                data=bytes(rgba_bytes),
                w=self.width,
                h=self.height,
                fmt="RGBA8",
            )
            idx = self.backend.cbv_srv_uav_heap.next_free()
            cpu = self.backend.cbv_srv_uav_heap.get_cpu_handle(idx)
            self.backend.create_shader_resource_view(self.tex, cpu)
            self._rtx_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(idx)
        else:
            self.backend.update_texture(self.tex, data=bytes(rgba_bytes), w=self.width, h=self.height)

        # 4️⃣ Draw fullscreen triangle
        self.backend.begin_frame()
        back_rtv = self.backend.rtv_heap.get_cpu_handle(0)
        self.backend.set_render_target(back_rtv)
        self.backend.clear_render_target(back_rtv, (0, 0, 0, 1))

        self.quad_shader.use()
        self.backend.set_root_descriptor_table(0, self._rtx_srv_gpu)

        self.backend.set_vertex_buffers(self.quad_vb)
        self.backend.draw(3)

        self.backend.end_frame()
        gl_check_error("[RTXRenderer] render")