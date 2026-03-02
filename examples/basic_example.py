# basic_example.py
from alkash3d import Engine, Window, select_backend

if __name__ == "__main__":
    # -------------------------------------------------
    # Окно + DX12‑бэкенд
    # -------------------------------------------------
    win = Window(800, 600, "Forward test")
    backend = select_backend("dx12")
    backend.init_device(win.hwnd, win.width, win.height)

    # -------------------------------------------------
    # Формируем меш с правильным layout‑ом
    # -------------------------------------------------
    import numpy as np
    from alkash3d.scene.mesh import Mesh

    verts = np.array([
        #   позиция          нормаль          UV
        -0.5, -0.5, 0.0,   0.0, 0.0, 1.0,   0.0, 0.0,   # V0
         0.5, -0.5, 0.0,   0.0, 0.0, 1.0,   1.0, 0.0,   # V1
         0.0,  0.5, 0.0,   0.0, 0.0, 1.0,   0.5, 1.0,   # V2
    ], dtype=np.float32)

    tri = Mesh(vertices=verts)   # Mesh автоматически разбивает массив на stride=32

    # -------------------------------------------------
    # Сцена + камера
    # -------------------------------------------------
    from alkash3d.scene import Scene, Camera
    scene = Scene()
    cam = Camera()
    scene.add_child(cam)
    scene.add_child(tri)

    # -------------------------------------------------
    # Одна отрисовка кадра
    # -------------------------------------------------
    backend.begin_frame()

    # Back‑buffer RTV (swap‑chain index 0 или 1)
    rtv = backend.rtv_heap.get_cpu_handle(backend.get_frame_index() % 2)
    backend.set_render_target(rtv)
    backend.clear_render_target(rtv, (0.1, 0.1, 0.2, 1.0))

    # ----------------- Шейдер -----------------
    from alkash3d.renderer.shader import Shader
    sh = Shader(
        vertex_path=str(win.resource_path("shaders/forward_vert.hlsl")),
        fragment_path=str(win.resource_path("shaders/forward_frag.hlsl")),
        backend=backend,
    )
    sh.use()   # активируем PSO

    # Uniform‑ы камеры
    sh.set_uniform_mat4("uView", cam.get_view_matrix())
    sh.set_uniform_mat4("uProj", cam.get_projection_matrix(800 / 600))

    # Если хотите изменить цвет (по умолчанию (1,1,1,1)):
    # sh.set_uniform_vec4("uTint", (1.0, 0.5, 0.3, 1.0))

    # ----------------- Рисуем -----------------
    tri.draw(backend)

    backend.end_frame()
    input("Press Enter to exit...")
