# alkash3d/utils/texture_loader.py
"""
Загружает PNG/JPG → DirectX 12‑текстуру, возвращает «resource‑handle».
"""

from pathlib import Path
import os
import numpy as np
from alkash3d.graphics.dx12_backend import DX12Backend
from alkash3d.utils.logger import logger

def _resolve_texture_path(path: str) -> Path:
    """
    Приводит пользовательский путь к абсолютному ``Path``.
    Поддерживает:

    * абсолютные пути – оставляем как есть;
    * относительные пути – ищем сначала в текущем рабочем каталоге
      (``os.getcwd()``), затем рядом с вызывающим скриптом
      (обычно `examples 2/`) и, в крайнем случае, в корне проекта
      (``<repo_root>/resources``).
    """
    p = Path(path)

    # 1️⃣ Абсолютный путь – ничего не делаем
    if p.is_absolute():
        return p.expanduser().resolve()

    # 2️⃣ Попытка отнести к текущему рабочему каталогу
    cwd_candidate = (Path.cwd() / p).resolve()
    if cwd_candidate.is_file():
        return cwd_candidate

    # 3️⃣ Попытка отнести к каталогу, где находится вызывающий скрипт.
    #    Это работает, когда пример запускается из `examples 2/`.
    #    ``__file__`` в этом модуле – находится в `alkash3d/utils`,
    #    поэтому поднимаемся на две уровнъ (utils → alkash3d → проект).
    script_dir = Path(__file__).parents[2]  # …/alkash3d/
    script_candidate = (script_dir / p).resolve()
    if script_candidate.is_file():
        return script_candidate

    # 4️⃣ Последний шанс – искать в корне проекта в папке `resources`.
    project_root = script_dir.parent  # …/ (корень репозитория)
    project_candidate = (project_root / "resources" / p).resolve()
    return project_candidate  # Если файл не существует – бросим ниже FileNotFoundError

def load_texture(path: str, backend: DX12Backend):
    """
    Загружает изображение через Pillow и создаёт DX12‑текстуру.
    Возвращаемый объект – указатель, полученный от ``backend.create_texture``.
    """
    if not isinstance(backend, DX12Backend):
        raise RuntimeError("[TextureLoader] DX12 backend required")

    # ---------------------------------------------------------------
    # 1️⃣ Приводим путь к абсолютному
    # ---------------------------------------------------------------
    p = _resolve_texture_path(path)

    # ---------------------------------------------------------------
    # 2️⃣ Если файл действительно не найден – бросаем понятную ошибку.
    # ---------------------------------------------------------------
    if not p.is_file():
        raise FileNotFoundError(f"Texture not found: {p}")

    # ---------------------------------------------------------------
    # 3️⃣ Читаем изображение и превращаем в RGBA‑байты
    # ---------------------------------------------------------------
    from PIL import Image   # импортируем только при необходимости
    img = Image.open(p).convert("RGBA")
    w, h = img.size
    img_data = np.array(img, dtype=np.uint8).tobytes()

    # ---------------------------------------------------------------
    # 4️⃣ Создаём DX12‑текстуру
    # ---------------------------------------------------------------
    tex = backend.create_texture(
        data=img_data,
        w=w,
        h=h,
        fmt="RGBA8",
    )

    # ---------------------------------------------------------------
    # 5️⃣ Создаём SRV‑дескриптор и сохраняем GPU‑хендл в объекте
    # ---------------------------------------------------------------
    idx = backend.cbv_srv_uav_heap.next_free()
    cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
    backend.create_shader_resource_view(tex, cpu_handle)
    tex._srv_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)

    logger.debug(f"[TextureLoader] Loaded texture {p} ({w}×{h})")
    return tex
