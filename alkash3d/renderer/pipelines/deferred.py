# alkash3d/renderer/pipelines/deferred.py
import ctypes
import numpy as np
from pathlib import Path
from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader
from alkash3d.utils import logger, gl_check_error
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight
from alkash3d.scene.mesh import Mesh
from alkash3d.culling.bvh import BVH
from alkash3d.graphics import select_backend

MAX_LIGHTS = 256

# -------------------------------------------------------------
# Корневой каталог проекта → resources/shaders
# -------------------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parents[3]   # …/AlKAsH3D-Engine
SHADER_DIR = PROJECT_ROOT / "resources" / "shaders"


class DeferredRenderer(BaseRenderer):
    """Deferred‑renderer с PBR‑G‑buffer и простым кластер‑lighting."""

    def __init__(self, window, backend=None):
        self.window = window
        self.backend = backend or select_backend("dx12")
        self.width, self.height = window.width, window.height

        # Geometry‑pass shaders
        self.geom_shader = Shader(
            vertex_path=str(SHADER_DIR / "deferred_geom_vert.hlsl"),
            fragment_path=str(SHADER_DIR / "deferred_geom_frag.hlsl"),
            backend=self.backend,
        )

        # Lighting‑pass shaders
        self.light_shader = Shader(
            vertex_path=str(SHADER_DIR / "deferred_light_vert.hlsl"),
            fragment_path=str(SHADER_DIR / "deferred_light_frag.hlsl"),
            backend=self.backend,
        )

        # G‑buffer (4 render‑targets)
        self._setup_gbuffer()
        self._setup_quad()
        self._setup_state()

        self.bvh = BVH()  # ускоритель (заглушка)

    # -----------------------------------------------------------------
    def _setup_gbuffer(self):
        """Создаём 4 render‑target‑textures и их RTV‑дескрипторы."""
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
                data=b"", w=self.width, h=self.height, fmt=fmt,
            )
            self.gbuffer_textures[name] = tex
            # создаём RTV‑дескриптор в rtv‑heap
            rtv_idx = self.backend.rtv_heap.next_free()
            rtv_handle = self.backend.rtv_heap.get_cpu_handle(rtv_idx)
            self.backend.create_render_target_view(tex, rtv_handle)
            self.rtv_handles.append(rtv_handle)

        # depth‑buffer (можно добавить отдельный DSV, но пока упрощаем)
        self.depth_tex = self.backend.create_texture(
            data=b"", w=self.width, h=self.height, fmt="D24_UNORM_S8_UINT"
        )
        # DSV‑дескриптор (необязательно в упрощённой реализации)

    # -----------------------------------------------------------------
    def _setup_quad(self):
        """Fullscreen‑quad для lighting‑pass."""
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
    def _setup_state(self):
        """Глобальные состояния (Depth‑test и т.д.)."""
        self.backend.enable_depth_test(True)

    # -----------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.width, self.height = w, h
        self._setup_gbuffer()    # recreate textures
        self._setup_quad()

    # -----------------------------------------------------------------
    def render(self, scene, camera):
        # -------------------------------------------------------------
        # 1️⃣ Geometry‑pass → G‑buffer
        # -------------------------------------------------------------
        self.backend.begin_frame()

        # привязываем все 4 RTV
        self.backend.set_render_targets(self.rtv_handles)

        self.geom_shader.use()
        self.geom_shader.set_uniform_mat4("uView", camera.get_view_matrix())
        self.geom_shader.set_uniform_mat4("uProj", camera.get_projection_matrix(self.width / self.height))

        # culling (упрощённый)
        cam_pos = camera.position.as_np()
        for node in scene.visible_nodes(camera):
            if not hasattr(node, "draw"):
                continue
            if isinstance(node, Mesh):
                centre, radius = node.bounding_sphere
                dist = np.linalg.norm(centre - cam_pos)
                if dist - radius > camera.far:
                    continue
                if dist + radius < camera.near:
                    continue

            model = node.get_world_matrix().to_gl()
            self.geom_shader.set_uniform_mat4("uModel", model)

            if hasattr(node, "material"):
                node.material.bind(self.backend)

            node.draw(self.backend)

        # -------------------------------------------------------------
        # 2️⃣ Lighting‑pass (fullscreen)
        # -------------------------------------------------------------
        # Пишем результат сразу в back‑buffer (swap‑chain RTV0)
        back_rtv = self.backend.rtv_heap.get_cpu_handle(0)
        self.backend.set_render_target(back_rtv)
        self.backend.clear_render_target(back_rtv, (0.07, 0.07, 0.08, 1.0))

        self.light_shader.use()
        self.light_shader.set_uniform_vec3("uCamPos", camera.position)

        # bind G‑buffer textures (SRV) – каждый SRV уже находится в cbv_srv_uav‑heap
        for i, name in enumerate(self.gbuffer_textures):
            tex = self.gbuffer_textures[name]
            gpu_handle = self.backend.cbv_srv_uav_heap.get_gpu_handle(i)
            self.backend.set_root_descriptor_table(i, gpu_handle)

        # bind lights
        lights = [
            n for n in scene.traverse()
            if isinstance(n, (DirectionalLight, PointLight, SpotLight))
        ]
        self.light_shader.set_uniform_int("uNumLights", min(len(lights), MAX_LIGHTS))
        for i, light in enumerate(lights[:MAX_LIGHTS]):
            uni = light.get_uniforms()
            prefix = f"lights[{i}]"
            self.light_shader.set_uniform_int(f"{prefix}.type", uni["type"])
            self.light_shader.set_uniform_vec3(f"{prefix}.color", uni["color"])
            self.light_shader.set_uniform_float(f"{prefix}.intensity", uni["intensity"])
            if uni["type"] == 0:  # directional
                self.light_shader.set_uniform_vec3(f"{prefix}.direction", uni["direction"])
            elif uni["type"] == 1:  # point
                self.light_shader.set_uniform_vec3(f"{prefix}.position", uni["position"])
                self.light_shader.set_uniform_float(f"{prefix}.radius", uni["radius"])
            elif uni["type"] == 2:  # spot
                self.light_shader.set_uniform_vec3(f"{prefix}.position", uni["position"])
                self.light_shader.set_uniform_vec3(f"{prefix}.spotDir", uni["direction"])
                self.light_shader.set_uniform_float(f"{prefix}.innerCutoff", uni["innerCutoff"])
                self.light_shader.set_uniform_float(f"{prefix}.outerCutoff", uni["outerCutoff"])

        # draw fullscreen triangle (lighting)
        self.backend.set_vertex_buffers(self.quad_vb)
        self.backend.draw(3)  # 3‑вершинный triangle

        self.backend.end_frame()
        gl_check_error("[DeferredRenderer] render")