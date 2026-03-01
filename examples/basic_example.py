# test_forward.py
from alkash3d import Engine, Window, select_backend

if __name__ == "__main__":
    # Окно + DX12‑бэкенд
    win = Window(800, 600, "Forward test")
    backend = select_backend("dx12")
    backend.init_device(win.hwnd, win.width, win.height)

    # Делаем простой треугольник
    import numpy as np
    from alkash3d.scene.mesh import Mesh

    verts = np.array(
        [
            -0.5, -0.5, 0.0,
            0.5, -0.5, 0.0,
            0.0, 0.5, 0.0,
        ],
        dtype=np.float32,
    )
    tri = Mesh(vertices=verts)
    scene = win.backend.scene if hasattr(win, "scene") else None
    # или создаём свою сцену:
    from alkash3d.scene import Scene, Camera
    scene = Scene()
    cam = Camera()
    scene.add_child(cam)
    scene.add_child(tri)

    # рендерим один кадр
    backend.begin_frame()
    # выбираем back‑buffer RTV
    rtv = backend.rtv_heap.get_cpu_handle(backend.get_frame_index() % 2)
    backend.set_render_target(rtv)
    backend.clear_render_target(rtv, (0.1, 0.1, 0.2, 1.0))

    # шейдер (путь к вашим .hlsl‑файлам)
    from alkash3d.renderer.shader import Shader
    sh = Shader(
        vertex_path=str(win.resource_path("shaders/forward_vert.hlsl")),
        fragment_path=str(win.resource_path("shaders/forward_frag.hlsl")),
        backend=backend,
    )
    sh.use()
    sh.set_uniform_mat4("uView", cam.get_view_matrix())
    sh.set_uniform_mat4("uProj", cam.get_projection_matrix(800 / 600))

    tri.draw(backend)
    backend.end_frame()
    input("Press Enter to exit...")
