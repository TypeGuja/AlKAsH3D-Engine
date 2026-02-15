# alkash3d/renderer/raytracer.py
"""
Простейший CUDA‑ray‑tracer → вывод в OpenGL‑texture → fullscreen‑quad.
"""

import math
import numpy as np
from numba import cuda
from OpenGL import GL
import ctypes
from pathlib import Path

from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader   # наш менеджер шейдеров


# -------------------------------------------------------------
@cuda.jit
def kernel_raytrace(width, height,
                    cam_pos, cam_dir, cam_up, cam_right,
                    out_image):
    """
    Элементарный ray‑marching‑kernel:
        - одна сфера (центр (0,0,0), радиус 1)
        - простой фон‑градиент
    """
    x, y = cuda.grid(2)
    if x >= width or y >= height:
        return

    ndc_x = (2.0 * x / width) - 1.0
    ndc_y = 1.0 - (2.0 * y / height)

    # направление луча в мировом пространстве
    ray_dir = cam_dir + ndc_x * cam_right + ndc_y * cam_up
    norm = math.sqrt(ray_dir[0] * ray_dir[0] +
                    ray_dir[1] * ray_dir[1] +
                    ray_dir[2] * ray_dir[2])
    ray_dir[0] /= norm
    ray_dir[1] /= norm
    ray_dir[2] /= norm

    # сфера в центре (0,0,0), радиус = 1
    oc = cam_pos
    a = ray_dir[0] * ray_dir[0] + ray_dir[1] * ray_dir[1] + ray_dir[2] * ray_dir[2]
    b = 2.0 * (oc[0] * ray_dir[0] + oc[1] * ray_dir[1] + oc[2] * ray_dir[2])
    c = oc[0] * oc[0] + oc[1] * oc[1] + oc[2] * oc[2] - 1.0
    disc = b * b - 4.0 * a * c

    if disc > 0.0:
        # попали – закрасим ярко‑оранжевым
        t = (-b - math.sqrt(disc)) / (2.0 * a)
        out_image[y, x, 0] = 255   # R
        out_image[y, x, 1] = 120   # G
        out_image[y, x, 2] = 30    # B
    else:
        # фон – тёмный градиент
        out_image[y, x, 0] = 30
        out_image[y, x, 1] = 30
        out_image[y, x, 2] = 80


# -------------------------------------------------------------
class RayTracer(BaseRenderer):
    """CUDA‑ядро → GL‑texture → fullscreen‑quad."""

    def __init__(self, window):
        self.window = window
        self._init_output_texture()

    # -----------------------------------------------------------------
    def _init_output_texture(self):
        w, h = self.window.width, self.window.height
        self.width, self.height = w, h
        self.image = np.zeros((h, w, 3), dtype=np.uint8)

        # OpenGL‑текстура, в которую будем копировать результат ядра
        self.tex_id = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.tex_id)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGB8,
                       w, h, 0, GL.GL_RGB, GL.GL_UNSIGNED_BYTE, None)
        GL.glTexParameteri(GL.GL_TEXTURE_2D,
                           GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D,
                           GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)

        # fullscreen‑quad VAO
        self.quad_vao = GL.glGenVertexArrays(1)
        GL.glBindVertexArray(self.quad_vao)

        verts = np.array([
            -1, -1, 0, 0,
             1, -1, 1, 0,
             1,  1, 1, 1,
            -1,  1, 0, 1,
        ], dtype=np.float32)

        vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, vbo)
        GL.glBufferData(GL.GL_ARRAY_BUFFER,
                        verts.nbytes, verts, GL.GL_STATIC_DRAW)

        GL.glEnableVertexAttribArray(0)
        GL.glVertexAttribPointer(0, 2, GL.GL_FLOAT, False,
                                4 * 4, ctypes.c_void_p(0))
        GL.glEnableVertexAttribArray(1)
        GL.glVertexAttribPointer(1, 2, GL.GL_FLOAT, False,
                                4 * 4, ctypes.c_void_p(2 * 4))

        # Шейдер, который просто выводит текстуру
        SHADER_DIR = Path(__file__).resolve().parents[2] / "resources" / "shaders"
        self.quad_shader = Shader(
            vertex_path=str(SHADER_DIR / "quad_vert.glsl"),
            fragment_path=str(SHADER_DIR / "quad_frag.glsl"),
        )

    # -----------------------------------------------------------------
    def resize(self, w: int, h: int):
        self.width, self.height = w, h
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.tex_id)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGB8,
                       w, h, 0, GL.GL_RGB, GL.GL_UNSIGNED_BYTE, None)

    # -----------------------------------------------------------------
    def render(self, scene, camera):
        """
        1️⃣ Запускаем CUDA‑ядро.
        2️⃣ Копируем результат в OpenGL‑текстуру.
        3️⃣ Рисуем fullscreen‑quad.
        """
        # 1️⃣ CUDA‑ядро
        threads = (16, 16)
        blocks_x = (self.width + threads[0] - 1) // threads[0]
        blocks_y = (self.height + threads[1] - 1) // threads[1]

        cam_pos = camera.position.as_np().astype(np.float32)
        cam_dir = camera.forward.as_np().astype(np.float32)
        cam_up = camera.up.as_np().astype(np.float32)
        cam_right = np.cross(cam_dir, cam_up).astype(np.float32)

        d_image = cuda.to_device(self.image)
        kernel_raytrace[(blocks_x, blocks_y), threads](
            self.width, self.height,
            cam_pos, cam_dir, cam_up, cam_right,
            d_image
        )
        d_image.copy_to_host(self.image)

        # 2️⃣ Копируем в GL‑текстуру
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.tex_id)
        GL.glTexSubImage2D(GL.GL_TEXTURE_2D, 0, 0, 0,
                           self.width, self.height,
                           GL.GL_RGB, GL.GL_UNSIGNED_BYTE, self.image)

        # 3️⃣ Вывод fullscreen‑quad
        GL.glClearColor(0.0, 0.0, 0.0, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)

        self.quad_shader.use()
        GL.glActiveTexture(GL.GL_TEXTURE0)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.tex_id)
        self.quad_shader.set_uniform_int("uInput", 0)

        GL.glBindVertexArray(self.quad_vao)
        GL.glDrawArrays(GL.GL_TRIANGLE_FAN, 0, 4)
        GL.glBindVertexArray(0)
