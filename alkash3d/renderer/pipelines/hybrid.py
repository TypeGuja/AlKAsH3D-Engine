# alkash3d/renderer/pipelines/hybrid.py
"""
Гибридный пайплайн: Deferred‑geom + CUDA/OptiX‑RT + пост‑процессинг.
Если native‑модуль rt_core недоступен – работает как обычный Deferred.
"""

import numpy as np
from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader
from alkash3d.scene.mesh import Mesh
from alkash3d.culling.bvh import BVH
from alkash3d.utils import logger, gl_check_error
from alkash3d.graphics import select_backend

# ───── Импортируем световые классы напрямую, чтобы разорвать цикл ─────
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight

# Пытаемся импортировать native‑модуль (CUDA/OptiX)
try:
    from alkash3d.native import rt_core
except Exception:
    rt_core = None
    logger.warning("[HybridRenderer] Native ray‑tracing module not available – will disable RT.")


class HybridRenderer(BaseRenderer):
    """Hybrid renderer (deferred geometry + optional ray‑tracing)."""

    def __init__(self, window, backend=None):
        self.window = window
        self.backend = backend or select_backend("dx12")
        self.width, self.height = window.width, window.height

        # 1️⃣ Geometry‑pass (PBR‑shader)
        self.geom_shader = Shader(
            vertex_path=str(window.resource_path("shaders/deferred_geom_vert.hlsl")),
            fragment_path=str(window.resource_path("shaders/deferred_geom_frag.hlsl")),
            backend=self.backend,
        )
        self.light_shader = Shader(
            vertex_path=str(window.resource_path("shaders/deferred_light_vert.hlsl")),
            fragment_path=str(window.resource_path("shaders/deferred_light_frag.hlsl")),
            backend=self.backend,
        )
        self._setup_gbuffer()
        self._setup_quad()
        self.backend.enable_depth_test(True)

        # 2️⃣ Ray‑tracer (CUDA/OptiX)
        self.rt_enabled = rt_core is not None
        if self.rt_enabled:
            self._init_raytracer_output()

        self.bvh = BVH()
        self.postproc = None  # будет заполнен в Engine

    # -----------------------------------------------------------------
    def _setup_gbuffer(self):
        # Переиспользуем ту же логику, что в DeferredRenderer
        fmt_map = {
            "position": "RGBA32F",
            "normal": "RGBA16F",
            "albedo": "RGBA8",
            "material": "RGBA8",
        }
        self.gbuffer_textures = {}
        self.rtv_handles = []

        for i, (name, fmt) in enumerate(fmt_map.items()):
            tex = self.backend.create_texture(
                data=b"", w=self.width, h=self.height, fmt=fmt
            )
            self.gbuffer_textures[name] = tex
            rtv_idx = self.backend.rtv_heap.next_free()
            rtv_handle = self.backend.rtv_heap.get_cpu_handle(rtv_idx)
            self.backend.create_render_target_view(tex, rtv_handle)
            self.rtv_handles.append(rtv_handle)

    # -----------------------------------------------------------------
    def _setup_quad(self):
        # Full‑screen triangle (3 verts)
        verts = np.array([-1.0, -1.0, 3.0, -1.0, -1.0, 3.0], dtype=np.float32)
        self.quad_vb = self.backend.create_buffer(verts.tobytes(), usage="vertex")

    # -----------------------------------------------------------------
    def _init_raytracer_output(self):
        """Текстура‑буфер, в которую записывает CUDA/OptiX."""
        self.rt_tex = self.backend.create_texture(
            data=b"", w=self.width, h=self.height, fmt="RGBA8"
        )
        # SRV в heap
        idx = self.backend.cbv_srv_uav_heap.next_free()
        cpu_handle = self.backend.cbv_srv_uav_heap.get_cpu_handle(idx)
        self.backend.create_shader_resource_view(self.rt_tex, cpu_handle)
        self.rt_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(idx)

    # -----------------------------------------------------------------
    def resize(self, w, h):
        self.width, self.height = w, h
        self._setup_gbuffer()
        if self.rt_enabled:
            # recreate RT texture
            self.rt_tex = self.backend.create_texture(
                data=b"", w=w, h=h, fmt="RGBA8"
            )
            idx = self.backend.cbv_srv_uav_heap.next_free()
            cpu_handle = self.backend.cbv_srv_uav_heap.get_cpu_handle(idx)
            self.backend.create_shader_resource_view(self.rt_tex, cpu_handle)
            self.rt_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(idx)

        if self.postproc:
            self.postproc.resize(w, h)

    # -----------------------------------------------------------------
    def render(self, scene, camera):
        # ---------- 1️⃣ Geometry‑pass ----------
        self.backend.begin_frame()
        self.backend.set_render_targets(self.rtv_handles)
        self.backend.clear_render_target(self.rtv_handles[0],
                                        (0.0, 0.0, 0.0, 1.0))

        self.geom_shader.use()
        self.geom_shader.set_uniform_mat4("uView", camera.get_view_matrix())
        self.geom_shader.set_uniform_mat4("uProj", camera.get_projection_matrix(self.width / self.height))

        for node in scene.visible_nodes(camera):
            if not hasattr(node, "draw"):
                continue
            model = node.get_world_matrix().to_gl()
            self.geom_shader.set_uniform_mat4("uModel", model)
            if hasattr(node, "material"):
                node.material.bind(self.backend)
            node.draw(self.backend)

        # ---------- 2️⃣ RT‑pass ----------
        if self.rt_enabled:
            meshes = [n for n in scene.traverse() if isinstance(n, Mesh)]
            self.bvh.build(meshes)

            rt_core.trace(
                width=self.width,
                height=self.height,
                cam_pos=camera.position.as_np(),
                cam_dir=camera.forward.as_np(),
                cam_up=camera.up.as_np(),
                cam_right=np.cross(camera.forward.as_np(),
                                   camera.up.as_np()),
                bvh=self.bvh,
                output_texture=self.rt_tex,
            )
            # Теперь rt_tex уже содержит результат – наш SRV уже готов.

        # ---------- 3️⃣ Lighting‑pass ----------
        back_rtv = self.backend.rtv_heap.get_cpu_handle(0)
        self.backend.set_render_target(back_rtv)
        self.backend.clear_render_target(back_rtv, (0.07, 0.07, 0.08, 1.0))

        self.light_shader.use()
        self.light_shader.set_uniform_vec3("uCamPos", camera.position)

        # bind G‑buffer textures + optional RT‑texture
        for i, name in enumerate(self.gbuffer_textures):
            tex = self.gbuffer_textures[name]
            gpu_handle = self.backend.cbv_srv_uav_heap.get_gpu_handle(i)
            self.backend.set_root_descriptor_table(i, gpu_handle)

        if self.rt_enabled:
            rt_slot = len(self.gbuffer_textures)
            self.backend.set_root_descriptor_table(rt_slot, self.rt_srv_gpu)

        # lights
        lights = [
            n for n in scene.traverse()
            if isinstance(n, (DirectionalLight, PointLight, SpotLight))
        ]
        self.light_shader.set_uniform_int("uNumLights", min(len(lights), 8))
        for i, light in enumerate(lights[:8]):
            uni = light.get_uniforms()
            pfx = f"lights[{i}]"
            self.light_shader.set_uniform_int(f"{pfx}.type", uni["type"])
            self.light_shader.set_uniform_vec3(f"{pfx}.color", uni["color"])
            self.light_shader.set_uniform_float(f"{pfx}.intensity", uni["intensity"])
            if uni["type"] == 0:
                self.light_shader.set_uniform_vec3(f"{pfx}.direction", uni["direction"])
            elif uni["type"] == 1:
                self.light_shader.set_uniform_vec3(f"{pfx}.position", uni["position"])
                self.light_shader.set_uniform_float(f"{pfx}.radius", uni["radius"])
            elif uni["type"] == 2:
                self.light_shader.set_uniform_vec3(f"{pfx}.position", uni["position"])
                self.light_shader.set_uniform_vec3(f"{pfx}.spotDir", uni["direction"])
                self.light_shader.set_uniform_float(f"{pfx}.innerCutoff", uni["innerCutoff"])
                self.light_shader.set_uniform_float(f"{pfx}.outerCutoff", uni["outerCutoff"])

        # draw fullscreen triangle (lighting)
        self.backend.set_vertex_buffers(self.quad_vb)
        self.backend.draw(3)

        # ---------- 4️⃣ Post‑process ----------
        if self.postproc:
            # postproc будет отрисовывать на back‑buffer, который уже
            # сейчас активен (swap‑chain RTV0)
            self.postproc.run(self.backend)

        self.backend.end_frame()
        gl_check_error("[HybridRenderer] render")
