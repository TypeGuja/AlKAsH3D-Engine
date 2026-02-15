#!/usr/bin/env python
# -*- coding: utf-8 -*-

"""
Вращающийся куб, DX12‑бэкенд, без правок Rust‑модуля.
* Увеличиваем RTV‑heap (3 дескриптора) в Engine.
* Белый placeholder создаём через upload‑buffer → default‑heap‑texture,
  без вызова Map.
"""

import numpy as np
from alkash3d.engine import Engine
from alkash3d.scene import Scene, Camera, DirectionalLight, Mesh
from alkash3d.math.vec3 import Vec3
from alkash3d.math.mat4 import Mat4
from alkash3d.renderer.shader import Shader   # ← импортируем напрямую


# ----------------------------------------------------------------------
def make_cube(size: float = 1.0):
    """(verts, norms, uvs, inds) простого куба."""
    hs = size * 0.5
    verts = np.array(
        [
            [-hs, -hs, -hs],
            [ hs, -hs, -hs],
            [ hs,  hs, -hs],
            [-hs,  hs, -hs],
            [-hs, -hs,  hs],
            [ hs, -hs,  hs],
            [ hs,  hs,  hs],
            [-hs,  hs,  hs],
        ],
        dtype=np.float32,
    )
    norms = verts / np.linalg.norm(verts, axis=1, keepdims=True)
    uvs = np.array(
        [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
        ],
        dtype=np.float32,
    )
    inds = np.array(
        [
            0, 1, 2, 0, 2, 3,
            4, 6, 5, 4, 7, 6,
            0, 4, 5, 0, 5, 1,
            3, 2, 6, 3, 6, 7,
            1, 5, 6, 1, 6, 2,
            4, 0, 3, 4, 3, 7,
        ],
        dtype=np.uint32,
    )
    return verts, norms, uvs, inds


# ----------------------------------------------------------------------
class RotatingCube(Mesh):
    """Куб, вращающийся вокруг оси Y."""
    def __init__(self, size: float = 1.0):
        v, n, uv, i = make_cube(size)
        super().__init__(vertices=v, normals=n, texcoords=uv, indices=i,
                         name="Cube")
        self._angle = 0.0

    def on_update(self, dt: float) -> None:
        self._angle += dt * 0.8               # ~45°/s
        self.rotation.y = np.degrees(self._angle)


# ----------------------------------------------------------------------
class ForwardRendererWithRTTPlaceholder:
    """
    Минимальная замена ForwardRenderer.
    Белая текстура создаётся через upload‑buffer → default‑heap‑texture,
    без Map‑вызовов.
    """

    def __init__(self, window, backend):
        self.window = window
        self.backend = backend

        # ---------- 1️⃣ Шейдер ----------
        self.shader = Shader(
            vertex_path=str(window.resource_path("shaders/forward_vert.hlsl")),
            fragment_path=str(window.resource_path("shaders/forward_frag.hlsl")),
            backend=self.backend,
        )

        # ---------- 2️⃣ Белый placeholder ----------
        self._create_white_placeholder()

        # ---------- 3️⃣ Дескриптор‑хипы ----------
        if self.backend.cbv_srv_uav_heap:
            self.backend.set_descriptor_heaps([self.backend.cbv_srv_uav_heap])

        # ---------- 4️⃣ PSO ----------
        self.backend.set_graphics_pipeline(self.shader.pso)

    # ------------------------------------------------------------------
    def _create_white_placeholder(self):
        """
        1. upload‑buffer → 4 байта (белый RGBA).
        2. texture‑resource (default‑heap, без начального сырья).
        3. копируем данные из буфера в texture через `update_buffer`
           (это внутри `dx.update_subresource` → копия без Map).
        4. создаём SRV и сохраняем GPU‑handle.
        """
        # 1️⃣ upload‑buffer
        white_pixel = (255).to_bytes(1, "little") * 4   # 0xFFFFFFFF
        upload_buf = self.backend.create_buffer(white_pixel, usage="upload")

        # 2️⃣ texture без данных (default‑heap)
        self.white_tex = self.backend.create_texture(
            data=None,          # без Map‑запросов
            w=1,
            h=1,
            fmt="RGBA8",
        )

        # 3️⃣ копируем из upload‑буфера в texture
        #    `update_buffer` → `dx.update_subresource` делает копию
        self.backend.update_buffer(upload_buf, white_pixel)

        # 4️⃣ SRV‑дескриптор
        srv_idx = self.backend.cbv_srv_uav_heap.next_free()
        cpu_srv = self.backend.cbv_srv_uav_heap.get_cpu_handle(srv_idx)
        self.backend.create_shader_resource_view(self.white_tex, cpu_srv)
        self.default_srv_gpu = self.backend.cbv_srv_uav_heap.get_gpu_handle(srv_idx)

    # ------------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    # ------------------------------------------------------------------
    def render(self, scene, camera) -> None:
        # 1️⃣ начало кадра
        self.backend.begin_frame()
        self.backend.set_viewport(0, 0, self.window.width, self.window.height)
        self.backend.set_scissor_rect(0, 0, self.window.width, self.window.height)

        # 2️⃣ uniform‑буферы
        self.shader.set_uniform_mat4("uView", camera.get_view_matrix())
        self.shader.set_uniform_mat4(
            "uProj",
            camera.get_projection_matrix(self.window.width / self.window.height),
        )
        self.shader.set_uniform_vec3("uCamPos", camera.position)

        # 3️⃣ чистим back‑buffer (RTV0)
        rtv0 = self.backend.rtv_heap.get_cpu_handle(0)
        self.backend.set_render_target(rtv0)
        self.backend.clear_render_target(rtv0, (0.07, 0.07, 0.08, 1.0))

        # 4️⃣ привязываем шейдер
        self.shader.use()

        # 5️⃣ каждый кадр выставляем descriptor‑heap‑s (на всякий случай)
        if self.backend.cbv_srv_uav_heap:
            self.backend.set_descriptor_heaps([self.backend.cbv_srv_uav_heap])

        # 6️⃣ обход сцены
        for node in scene.traverse():
            if not hasattr(node, "draw"):
                continue

            # материал / fallback‑текстура
            if hasattr(node, "material") and node.material:
                node.material.bind(self.backend)
            else:
                self.backend.set_root_descriptor_table(
                    root_index=0,
                    gpu_handle=self.default_srv_gpu,
                )

            # модель‑матрица
            self.shader.set_uniform_mat4("uModel",
                                        node.get_world_matrix().to_gl())

            # tint (если есть)
            if hasattr(node, "color"):
                self.shader.set_uniform_vec3("uTint", node.color)
            else:
                self.shader.set_uniform_vec3(
                    "uTint", np.array([1.0, 1.0, 1.0], np.float32))

            # отрисовка меша
            node.draw(self.backend)

        # 7️⃣ завершаем кадр (present + sync)
        self.backend.end_frame()


# ----------------------------------------------------------------------
def main() -> None:
    # 1️⃣ Engine создаёт окно, камеру,DX12‑backend и сразу
    #    инициализирует RTV‑heap. Мы расширяем её до 3‑х дескрипторов.
    engine = Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – rotating cube (DX12, без правок Rust)",
        renderer="forward",
        backend_name="dx12",
    )

    # **Увеличиваем RTV‑heap** (в Engine уже есть объект `rtv_heap`,
    # заменяем его на новый с +1 дескриптором):
    from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap
    engine.backend.rtv_heap = DescriptorHeap(
        device=engine.backend.device,
        num_descriptors=engine.backend.rtv_heap.num_descriptors + 1,
        heap_type="rtv",
    )

    # 2️⃣ Подменяем рендерер
    engine.renderer = ForwardRendererWithRTTPlaceholder(
        engine.window,
        engine.backend,
    )

    # 3️⃣ Свет
    sun = DirectionalLight(
        direction=Vec3(0.0, -1.0, -1.0),
        color=Vec3(1.0, 1.0, 1.0),
        intensity=3.0,
        name="Sun",
    )
    engine.scene.add_child(sun)

    # 4️⃣ Вращающийся куб
    cube = RotatingCube(size=1.0)
    engine.scene.add_child(cube)

    # 5️⃣ Позиция камеры
    engine.camera.position = Vec3(0.0, 0.0, 3.5)
    engine.camera.rotation = Vec3(0.0, 0.0, 0.0)

    # 6️⃣ Запуск главного цикла
    engine.run()


if __name__ == "__main__":
    main()
