# alkash3d/renderer/pipelines/deferred.py
# ---------------------------------------------------------------
# Deferred‑renderer (G‑buffer + lighting‑pass).
# ---------------------------------------------------------------
import ctypes

from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader
from alkash3d.utils.logger import gl_check_error
from OpenGL import GL
import numpy as np
from pathlib import Path

# Световые классы – импортируем напрямую, без круговых зависимостей
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight

MAX_LIGHTS = 8

# Путь к шейдерам (два уровня вверх от текущего файла → корень пакета)
SHADER_DIR = Path(__file__).resolve().parents[2] / "resources" / "shaders"

class DeferredRenderer(BaseRenderer):
    def __init__(self, window):
        self.window = window
        self.width, self.height = window.width, window.height

        # Geometry‑pass шейдеры
        self.geom_shader = Shader(
            vertex_path=str(SHADER_DIR / "deferred_geom_vert.glsl"),
            fragment_path=str(SHADER_DIR / "deferred_geom_frag.glsl"),
        )
        # Lighting‑pass шейдеры
        self.light_shader = Shader(
            vertex_path=str(SHADER_DIR / "deferred_light_vert.glsl"),
            fragment_path=str(SHADER_DIR / "deferred_light_frag.glsl"),
        )

        self._setup_gbuffer()
        self._setup_quad()
        self._setup_state()
    # -----------------------------------------------------------------
    # G‑buffer
    # -----------------------------------------------------------------
    def _setup_gbuffer(self):
        self.gbuffer_fbo = GL.glGenFramebuffers(1)
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, self.gbuffer_fbo)

        # ---------- Position (RGB32F) ----------
        self.pos_tex = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.pos_tex)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGB32F,
                        self.width, self.height, 0,
                        GL.GL_RGB, GL.GL_FLOAT, None)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)
        GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER, GL.GL_COLOR_ATTACHMENT0,
                                 GL.GL_TEXTURE_2D, self.pos_tex, 0)

        # ---------- Normal (RGB16F) ----------
        self.norm_tex = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.norm_tex)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGB16F,
                        self.width, self.height, 0,
                        GL.GL_RGB, GL.GL_FLOAT, None)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)
        GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER, GL.GL_COLOR_ATTACHMENT1,
                                 GL.GL_TEXTURE_2D, self.norm_tex, 0)

        # ---------- Albedo + Specular (RGBA8) ----------
        self.alb_spec_tex = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.alb_spec_tex)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGBA8,
                        self.width, self.height, 0,
                        GL.GL_RGBA, GL.GL_UNSIGNED_BYTE, None)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)
        GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER, GL.GL_COLOR_ATTACHMENT2,
                                 GL.GL_TEXTURE_2D, self.alb_spec_tex, 0)

        # ---------- Depth buffer ----------
        self.depth_rbo = GL.glGenRenderbuffers(1)
        GL.glBindRenderbuffer(GL.GL_RENDERBUFFER, self.depth_rbo)
        GL.glRenderbufferStorage(GL.GL_RENDERBUFFER,
                                GL.GL_DEPTH_COMPONENT24,
                                self.width, self.height)
        GL.glFramebufferRenderbuffer(GL.GL_FRAMEBUFFER,
                                    GL.GL_DEPTH_ATTACHMENT,
                                    GL.GL_RENDERBUFFER,
                                    self.depth_rbo)

        # указываем, какие буферы будем рисовать
        draw_buffers = [GL.GL_COLOR_ATTACHMENT0,
                        GL.GL_COLOR_ATTACHMENT1,
                        GL.GL_COLOR_ATTACHMENT2]
        GL.glDrawBuffers(len(draw_buffers), draw_buffers)

        # проверка
        if GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER) != GL.GL_FRAMEBUFFER_COMPLETE:
            raise RuntimeError("G‑buffer incomplete!")
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, 0)

    # -----------------------------------------------------------------
    # Fullscreen‑quad (для lighting‑pass)
    # -----------------------------------------------------------------
    def _setup_quad(self):
        self.quad_vao = GL.glGenVertexArrays(1)
        GL.glBindVertexArray(self.quad_vao)

        quad_vertices = np.array([
            # positions   # texcoords
            -1.0, -1.0,   0.0, 0.0,
             1.0, -1.0,   1.0, 0.0,
             1.0,  1.0,   1.0, 1.0,
            -1.0,  1.0,   0.0, 1.0,
        ], dtype=np.float32)

        quad_vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, quad_vbo)
        GL.glBufferData(GL.GL_ARRAY_BUFFER, quad_vertices.nbytes,
                        quad_vertices, GL.GL_STATIC_DRAW)

        # позиция
        GL.glEnableVertexAttribArray(0)
        GL.glVertexAttribPointer(0, 2, GL.GL_FLOAT, False, 4 * 4, ctypes.c_void_p(0))
        # texcoord
        GL.glEnableVertexAttribArray(1)
        GL.glVertexAttribPointer(1, 2, GL.GL_FLOAT, False, 4 * 4, ctypes.c_void_p(2 * 4))

        GL.glBindVertexArray(0)

    # -----------------------------------------------------------------
    # OpenGL state
    # -----------------------------------------------------------------
    def _setup_state(self):
        GL.glEnable(GL.GL_DEPTH_TEST)

    # -----------------------------------------------------------------
    # Resize
    # -----------------------------------------------------------------
    def resize(self, w, h):
        self.width, self.height = w, h
        self._setup_gbuffer()          # пересоздаём G‑buffer

    # -----------------------------------------------------------------
    # Render
    # -----------------------------------------------------------------
    def render(self, scene, camera):
        # ---------- 1️⃣ Geometry pass ----------
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, self.gbuffer_fbo)
        GL.glViewport(0, 0, self.width, self.height)
        GL.glClearColor(0.0, 0.0, 0.0, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)

        self.geom_shader.reload_if_needed()
        self.geom_shader.use()
        view = camera.get_view_matrix()                     # numpy‑array
        proj = camera.get_projection_matrix(self.width / self.height)  # numpy‑array
        self.geom_shader.set_uniform_mat4("uView", view)
        self.geom_shader.set_uniform_mat4("uProj", proj)

        for node in scene.traverse():
            if hasattr(node, "draw"):
                # <-- важное изменение: .to_np()
                model = node.get_world_matrix().to_gl()
                self.geom_shader.set_uniform_mat4("uModel", model)
                node.draw()

        # ---------- 2️⃣ Lighting pass ----------
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, 0)   # default FB
        GL.glClearColor(0.07, 0.07, 0.08, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT)

        self.light_shader.reload_if_needed()
        self.light_shader.use()
        # позиция камеры (Vec3 → numpy)
        self.light_shader.set_uniform_vec3("uCamPos", camera.position.as_np())

        # кол‑во светов (по‑прежнему хвостовой массив, но передаём реальное число)
        self.light_shader.set_uniform_int("uNumLights",
                                         min(len([n for n in scene.traverse()
                                                  if isinstance(n, (DirectionalLight,
                                                                   PointLight,
                                                                   SpotLight))]),
                                         MAX_LIGHTS))

        # bind G‑buffer textures
        GL.glActiveTexture(GL.GL_TEXTURE0)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.pos_tex)
        self.light_shader.set_uniform_int("gPosition", 0)

        GL.glActiveTexture(GL.GL_TEXTURE1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.norm_tex)
        self.light_shader.set_uniform_int("gNormal", 1)

        GL.glActiveTexture(GL.GL_TEXTURE2)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.alb_spec_tex)
        self.light_shader.set_uniform_int("gAlbedoSpec", 2)

        # ------------ собрать массив lights -----------------
        lights = [
            node for node in scene.traverse()
            if isinstance(node, (DirectionalLight, PointLight, SpotLight))
        ]

        for i in range(MAX_LIGHTS):
            if i < len(lights):
                uni = lights[i].get_uniforms()
                self.light_shader.set_uniform_int(f"lights[{i}].type", uni["type"])
                self.light_shader.set_uniform_vec3(f"lights[{i}].color", uni["color"])
                self.light_shader.set_uniform_float(f"lights[{i}].intensity", uni["intensity"])

                if uni["type"] == 0:               # Directional
                    self.light_shader.set_uniform_vec3(f"lights[{i}].direction",
                                                      uni["direction"])
                elif uni["type"] == 1:             # Point
                    self.light_shader.set_uniform_vec3(f"lights[{i}].position",
                                                      uni["position"])
                    self.light_shader.set_uniform_float(f"lights[{i}].radius",
                                                      uni["radius"])
                elif uni["type"] == 2:             # Spot
                    self.light_shader.set_uniform_vec3(f"lights[{i}].position",
                                                      uni["position"])
                    # в шейдере ожидаем поле spotDir
                    self.light_shader.set_uniform_vec3(f"lights[{i}].spotDir",
                                                      uni["direction"])
                    self.light_shader.set_uniform_float(f"lights[{i}].innerCutoff",
                                                      uni["innerCutoff"])
                    self.light_shader.set_uniform_float(f"lights[{i}].outerCutoff",
                                                      uni["outerCutoff"])
            else:
                self.light_shader.set_uniform_int(f"lights[{i}].type", -1)

        # -------------- fullscreen‑quad ---------------
        GL.glBindVertexArray(self.quad_vao)
        GL.glDrawArrays(GL.GL_TRIANGLE_FAN, 0, 4)
        GL.glBindVertexArray(0)

        gl_check_error("DeferredRenderer.render")