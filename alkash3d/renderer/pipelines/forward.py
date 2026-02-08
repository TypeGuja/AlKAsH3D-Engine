# alkash3d/renderer/pipelines/forward.py
# ---------------------------------------------------------------
# Forward‑renderer с фиксированным массивом MAX_LIGHTS = 8.
# ---------------------------------------------------------------
from __future__ import annotations
from pathlib import Path
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight

from ..base_renderer import BaseRenderer
from ..shader import Shader
from ...utils.logger import gl_check_error
from OpenGL import GL

MAX_LIGHTS = 8

# Путь к каталогу, где лежат шейдеры (от уровня pipelines)
SHADER_DIR = Path(__file__).resolve().parents[2] / "resources" / "shaders"


class ForwardRenderer(BaseRenderer):
    """
    Простой forward‑pipeline.
    При отсутствии реальной текстуры автоматически создаётся 1×1‑белая
    текстура‑заглушка, а uniform‑sampler `uAlbedo` привязывается к
    текстурному юниту 0.
    """

    def __init__(self, window):
        self.window = window

        # ---------------------------------------------------------
        # Шейдер
        # ---------------------------------------------------------
        self.shader = Shader(
            vertex_path=str(SHADER_DIR / "forward_vert.glsl"),
            fragment_path=str(SHADER_DIR / "forward_frag.glsl"),
        )
        self._setup_state()

        # ---------------------------------------------------------
        # Белая текстура‑заглушка (1×1, полностью белая)
        # ---------------------------------------------------------
        self._create_default_white_texture()

        # ---------------------------------------------------------
        # Установим uniform‑sampler `uAlbedo` → текстурный юнит 0
        # ---------------------------------------------------------
        self.shader.use()
        self.shader.set_uniform_int("uAlbedo", 0)          # sampler = unit 0
        # По‑умолчанию будем использовать fallback‑цвет (белый)
        self.shader.set_uniform_int("uUseTexture", 0)

    # -----------------------------------------------------------------
    # OpenGL‑state (depth‑test)
    # -----------------------------------------------------------------
    def _setup_state(self) -> None:
        GL.glEnable(GL.GL_DEPTH_TEST)
        GL.glDepthFunc(GL.GL_LEQUAL)
        gl_check_error("ForwardRenderer._setup_state")

    # -----------------------------------------------------------------
    # Белая 1×1 текстура‑заглушка
    # -----------------------------------------------------------------
    def _create_default_white_texture(self) -> None:
        """
        Создаёт 1×1 белую текстуру и привязывает её к texture unit 0.
        """
        self.default_tex = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.default_tex)

        # 1×1 белый пиксель (RGBA = 255,255,255,255)
        white_pixel = (255).to_bytes(1, "little") * 4
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGBA8,
                        1, 1, 0,
                        GL.GL_RGBA, GL.GL_UNSIGNED_BYTE, white_pixel)

        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)

        # Привязываем к юниту 0
        GL.glActiveTexture(GL.GL_TEXTURE0)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.default_tex)

    # -----------------------------------------------------------------
    # Resize‑callback
    # -----------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        GL.glViewport(0, 0, w, h)

    # -----------------------------------------------------------------
    # Основной рендер‑проход
    # -----------------------------------------------------------------
    def render(self, scene, camera) -> None:
        """
        Упрощённая версия рендера без сложной системы освещения.
        """
        self.shader.reload_if_needed()
        self.shader.use()

        # Установка uniform-ов камеры
        view = camera.get_view_matrix()
        proj = camera.get_projection_matrix(self.window.width / self.window.height)
        self.shader.set_uniform_mat4("uView", view)
        self.shader.set_uniform_mat4("uProj", proj)
        self.shader.set_uniform_vec3("uCamPos", camera.position.as_np())
        self.shader.set_uniform_int("uUseTexture", 0)

        # Очистка буфера
        GL.glClearColor(0.07, 0.07, 0.08, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)

        # Рендеринг объектов сцены
        for node in scene.traverse():
            if hasattr(node, "draw"):
                model = node.get_world_matrix().to_gl()
                self.shader.set_uniform_mat4("uModel", model)
                node.draw()

        gl_check_error("ForwardRenderer.render")