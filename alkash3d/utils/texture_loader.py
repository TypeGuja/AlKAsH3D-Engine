"""
Загружает PNG/JPG → DirectX 12‑текстуру, возвращает «resource‑handle».
"""

from pathlib import Path
from PIL import Image
import numpy as np
from alkash3d.graphics.dx12_backend import DX12Backend
from alkash3d.utils.logger import logger

def load_texture(path: str, backend: DX12Backend):
    """
    Загружает изображение через Pillow и создаёт DX12‑текстуру.
    Возвращаемый объект – указатель, полученный от backend.create_texture.
    """
    if not isinstance(backend, DX12Backend):
        raise RuntimeError("[TextureLoader] DX12 backend required")

    p = Path(path).expanduser().resolve()
    if not p.is_file():
        raise FileNotFoundError(f"Texture not found: {p}")

    img = Image.open(p).convert("RGBA")
    w, h = img.size
    img_data = np.array(img, dtype=np.uint8).tobytes()

    tex = backend.create_texture(
        data=img_data,
        w=w,
        h=h,
        fmt="RGBA8",
    )

    idx = backend.cbv_srv_uav_heap.next_free()
    cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
    backend.create_shader_resource_view(tex, cpu_handle)
    tex._srv_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)

    logger.debug(f"[TextureLoader] Loaded texture {p} ({w}x{h})")
    return tex