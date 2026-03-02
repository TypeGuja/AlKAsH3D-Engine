# examples/basic_example.py
from alkash3d import Engine, Window, select_backend

if __name__ == "__main__":
    # Окно 800×600
    win = Window(800, 600, "AlKAsH3D Forward Demo")

    # Выбираем DX12‑бэкенд (теперь он будет работать)
    backend = select_backend("dx12")
    backend.init_device(win.hwnd, win.width, win.height)

    # Создаём движок с forward‑renderer
    engine = Engine(
        width=800,
        height=600,
        title="Forward Demo",
        renderer="forward",   # используем ForwardRenderer
        backend_name="dx12",
    )

    # Добавляем простейший треугольник (по умолчанию он будет белым)
    import numpy as np
    from alkash3d.scene.mesh import Mesh

    verts = np.array(
        [
            -0.5, -0.5, 0.0,
            0.5, -0.5, 0.0,
            0.0,  0.5, 0.0,
        ],
        dtype=np.float32,
    )
    tri = Mesh(vertices=verts)
    engine.scene.add_child(tri)   # помещаем в сцену

    # Включаем яркий цвет, чтобы было видно
    # (в ForwardRenderer.render() можно добавить эту строку;
    #  здесь делаем это сразу после инициализации шейдера)
    # engine.renderer.shader.set_uniform_vec4("uTint", (1.0, 0.8, 0.4, 1.0))

    # Запускаем основной цикл
    engine.run()