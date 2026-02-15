# example_game.py
# --------------------------------------------------------------
# Пример простого приложения, показывающего одну текстуру.
# --------------------------------------------------------------

import os
import numpy as np
from alkash3d import Engine
from alkash3d.scene import Mesh
from alkash3d.assets.material import PBRMaterial
from alkash3d.utils import logger

# ------------------------------------------------------------------
# 1️⃣ Создаём движок (внутри он сразу создаёт объект Window)
# ------------------------------------------------------------------
engine = Engine(
    width=1280,
    height=720,
    title="AlKAsH3D – Image Demo",
    renderer="forward",      # Forward‑pipeline
    backend_name="dx12",    # DirectX 12‑бэкенд
)

# ------------------------------------------------------------------
# 2️⃣ Путь к текстуре через Window.resource_path()
# ------------------------------------------------------------------
TEXTURE_PATH = engine.window.resource_path("textures/logo.png")

# ------------------------------------------------------------------
# 3️⃣ Если файл не найден – автоматически генерируем PNG‑заглушку
# ------------------------------------------------------------------
if not os.path.isfile(TEXTURE_PATH):
    os.makedirs(os.path.dirname(TEXTURE_PATH), exist_ok=True)

    # ---- простая PNG‑заглушка -----------------
    from PIL import Image, ImageDraw

    def create_placeholder_texture(path: str, size: int = 256) -> None:
        img = Image.new("RGBA", (size, size), (30, 30, 120, 255))
        draw = ImageDraw.Draw(img)
        txt = "AlKAsH3D"
        bbox = draw.textbbox((0, 0), txt)
        txt_w = bbox[2] - bbox[0]
        txt_h = bbox[3] - bbox[1]
        draw.text(((size - txt_w) / 2, (size - txt_h) / 2),
                  txt, fill=(255, 200, 50, 255))
        img.save(path, "PNG")

    create_placeholder_texture(TEXTURE_PATH)
    logger.info(f"[Example] Сгенерирована заглушка‑текстура → {TEXTURE_PATH}")

# ------------------------------------------------------------------
# 4️⃣ Создаём простой квадрат (2‑треугольника) с UV‑координатами
# ------------------------------------------------------------------
verts = np.array([
    -1.0, -1.0, 0.0,
     1.0, -1.0, 0.0,
    -1.0,  1.0, 0.0,
     1.0,  1.0, 0.0,
], dtype=np.float32)

uvs = np.array([
    0.0, 0.0,
    1.0, 0.0,
    0.0, 1.0,
    1.0, 1.0,
], dtype=np.float32)

indices = np.array([0, 1, 2, 2, 1, 3], dtype=np.uint32)

quad = Mesh(
    vertices=verts.reshape(-1, 3),
    texcoords=uvs.reshape(-1, 2),
    indices=indices,
    name="Quad",
)

# ------------------------------------------------------------------
# 5️⃣ Материал, указывающий на нашу текстуру
# ------------------------------------------------------------------
material = PBRMaterial(
    albedo=(1.0, 1.0, 1.0, 1.0),
    albedo_map=TEXTURE_PATH,
)

quad.material = material

# ------------------------------------------------------------------
# 6️⃣ Добавляем объект в сцену и запускаем основной цикл
# ------------------------------------------------------------------
engine.scene.add_child(quad)
logger.info("[Example] Запускаем главный цикл")
engine.run()
