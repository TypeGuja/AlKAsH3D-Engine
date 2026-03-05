# basic_example.py
# -------------------------------------------------
# Пример «ручного» рендеринга через DX12‑бэкенд.
# -------------------------------------------------
from alkash3d import Window, select_backend

if __name__ == "__main__":
    # -------------------------------------------------
    # 1️⃣ Окно + DX12‑бэкенд
    # -------------------------------------------------
    win = Window(800, 600, "Forward test")
    backend = select_backend("dx12")
    backend.init_device(win.hwnd, win.width, win.height)

    # -------------------------------------------------
    # 2️⃣ Формируем меш (позиция + нормаль + UV)
    # -------------------------------------------------
    import numpy as np
    from alkash3d.scene.mesh import Mesh

    verts = np.array([
        #  позиция           нормаль          UV
        -0.5, -0.5, 0.0,    0.0, 0.0, 1.0,   0.0, 0.0,   # V0
         0.5, -0.5, 0.0,    0.0, 0.0, 1.0,   1.0, 0.0,   # V1
         0.0,  0.5, 0.0,    0.0, 0.0, 1.0,   0.5, 1.0,   # V2
    ], dtype=np.float32)

    tri = Mesh(vertices=verts)   # Mesh сам разобьёт массив на stride=32

    # -------------------------------------------------
    # 3️⃣ Сцена + камера
    # -------------------------------------------------
    from alkash3d.scene import Scene, Camera
    scene = Scene()
    cam = Camera()
    scene.add_child(cam)
    scene.add_child(tri)

    # -------------------------------------------------
    # 4️⃣ Один кадр «вручную»
    # -------------------------------------------------
    backend.begin_frame()

    # 4.1️⃣ Back‑buffer (RTV) и очистка
    rtv = backend.rtv_heap.get_cpu_handle(
        backend.get_frame_index() % 2)            # 0 или 1
    backend.set_render_target(rtv)
    backend.clear_render_target(rtv, (0.1, 0.1, 0.2, 1.0))

    # 4.2️⃣ Шейдер
    from alkash3d.renderer.shader import Shader
    sh = Shader(
        vertex_path=str(win.resource_path("shaders/forward_vert.hlsl")),
        fragment_path=str(win.resource_path("shaders/forward_frag.hlsl")),
        backend=backend,
    )
    sh.use()                                     # ставим PSO

    # 4.3️⃣ Uniform‑ы камеры
    sh.set_uniform_mat4("uView", cam.get_view_matrix())
    sh.set_uniform_mat4("uProj", cam.get_projection_matrix(800 / 600))

    # **ВАЖНО** – отправляем данные в GPU
    sh.flush()                                   # копируем CB и ставим root‑descriptor

    # 4.4️⃣ Привязываем descriptor‑heaps.
    # Один RTV‑heap (для back‑buffer) и один CBV/SRV/UAV‑heap (для CB).
    backend.set_descriptor_heaps([backend.rtv_heap, backend.cbv_srv_uav_heap])

    # 4.5️⃣ Рисуем геометрию
    tri.draw(backend)

    # 4.6️⃣ Завершаем кадр
    backend.end_frame()

    # 4.7️⃣ Present – показываем результат
    backend.present()            # чисто DX12‑present (sync‑interval = 1)

    # -------------------------------------------------
    # 5️⃣ Ждём ввода, чтобы окно не закрылось сразу
    # -------------------------------------------------
    input("Press Enter to exit...")
