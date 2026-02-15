# examples 2/main.py  (корректный фрагмент)
import numpy as np

from alkash3d.engine import Engine
from alkash3d.graphics.utils import DescriptorHeap
from alkash3d.scene import Scene, Camera, DirectionalLight, Mesh
from alkash3d.math.vec3 import Vec3
from alkash3d.renderer.pipelines.forward import ForwardRenderer  # <- будем заменять
from alkash3d.graphics.utils import *
from debug_test import make_cube

# -------------------------------------------------------------------------
# 1️⃣  Создаём движок как обычно
engine = Engine(
    width=1280,
    height=720,
    title="AlKAsH3D – rotating cube (DX12, fixed RTV)",
    renderer="forward",               # используем наш кастомный рендерер позже
    backend_name="dx12",
)

# -------------------------------------------------------------------------
# 2️⃣  **Увеличиваем** размер RTV‑heap уже *после* инициализации бекенда,
#      но **сразу же** создаём RTV‑дескрипторы для swap‑chain.
engine.backend.rtv_heap = DescriptorHeap(
    device=engine.backend.device,
    num_descriptors=dx.SWAP_CHAIN_BUFFER_COUNT + 2,  # +1 для будущих RTV, +1 reserve
    heap_type="rtv",
)

# Пересоздаём RTV‑дескрипторы (тот же код, что в DX12Backend._create_swapchain_rtv)
engine.backend._create_swapchain_rtv()   # <-- важный шаг!

# -------------------------------------------------------------------------
# 3️⃣  Подменяем стандартный ForwardRenderer на наш кастомный
from alkash3d import ForwardRenderer   # ваш файл
engine.renderer = ForwardRenderer(engine.window, engine.backend)

# -------------------------------------------------------------------------
# 4️⃣  Добавляем свет и вращающийся куб (как в оригинальном примере)
sun = DirectionalLight(direction=Vec3(0.0, -1.0, -1.0), intensity=3.0)
engine.scene.add_child(sun)

class RotatingCube(Mesh):
    def __init__(self, size=1.0):
        v, n, uv, i = make_cube(size)      # функция из примера
        super().__init__(vertices=v, normals=n, texcoords=uv, indices=i, name="Cube")
        self._angle = 0.0

    def on_update(self, dt):
        self._angle += dt * 0.8
        self.rotation.y = np.degrees(self._angle)

engine.scene.add_child(RotatingCube())

engine.camera.position = Vec3(0.0, 0.0, 3.5)

# -------------------------------------------------------------------------
# 5️⃣  Запускаем цикл
engine.run()