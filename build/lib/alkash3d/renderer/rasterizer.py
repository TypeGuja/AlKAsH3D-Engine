# alkash3d/renderer/rasterizer.py
# ---------------------------------------------------------------
# OpenGL‑fallback‑renderer (использует default‑шэйдеры).
# ---------------------------------------------------------------
from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader
from OpenGL import GL
from pathlib import Path

class Rasterizer(BaseRenderer):
    """Минимальный forward‑renderer, использующий один шейдер."""

    def __init__(self, window):
        self.window = window
        SHADER_DIR = Path(__file__).resolve().parents[2] / "resources" / "shaders"
        self.shader = Shader(
            vertex_path=str(SHADER_DIR / "default_vert.glsl"),
            fragment_path=str(SHADER_DIR / "default_frag.glsl"),
        )
        self._setup_state()

    def _setup_state(self):
        GL.glEnable(GL.GL_DEPTH_TEST)

    def resize(self, w, h):
        GL.glViewport(0, 0, w, h)

    def render(self, scene, camera):
        self.shader.reload_if_needed()
        self.shader.use()

        view = camera.get_view_matrix()
        proj = camera.get_projection_matrix(self.window.width / self.window.height)
        self.shader.set_uniform_mat4("uView", view)
        self.shader.set_uniform_mat4("uProj", proj)

        GL.glClearColor(0.1, 0.1, 0.12, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)

        for node in scene.traverse():
            if hasattr(node, "draw"):
                model = node.get_world_matrix().to_gl()
                self.shader.set_uniform_mat4("uModel", model)
                node.draw()