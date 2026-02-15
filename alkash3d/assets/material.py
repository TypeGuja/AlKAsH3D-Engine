# -*- coding: utf-8 -*-
"""
PBR‑материал – хранит параметры (albedo, metallic, roughness, ao,
emissive) и ссылки на DX12‑текстуры.

В отличие от прежней реализации, здесь:

1️⃣  Текстуры **загружаются** (по‑запросу) в методе
    `_ensure_textures`.  Этот метод вызывается в начале `bind`,
    поэтому материал гарантировано имеет готовый `DX12Texture`.

2️⃣  Для материала **не создаётся свой CBV** – все матрицы передаются
    через единственный `constant‑buffer`, который создаёт `Shader`
    (`Shader._frame_cb`).  Поскольку в текущем `forward`‑шейдере
    параметры материала не используются, отдельный CBV не нужен.
    (Если в будущих шейдерах понадобится отдельный буфер,
    его можно добавить, но сейчас – лишний оверхед).

3️⃣  После загрузки текстуры создаётся **SRV‑дескриптор** в heap‑е
    `cbv_srv_uav_heap`, а затем вызывается
    `backend.set_root_descriptor_table(1, gpu_handle)`.  Слот 1
    соответствует **SRV** в корневой подписи (CBV – slot 0,
    SRV – slot 1).

Таким образом `bind()` теперь действительно привязывает вашу
текстуру к шейдеру, а чёрный экран исчезает.
"""

from __future__ import annotations

import numpy as np

from alkash3d.utils import logger
from alkash3d.utils.texture_loader import load_texture
from alkash3d.graphics.dx12_backend import DX12Backend


class PBRMaterial:
    """
    Хранит параметры PBR‑материала и ссылки на DX12‑текстуры.
    Привязывает только **одну** текстуру (по‑умолчанию – albedo‑map).
    """

    # Уникальный “binding point” – пока только для отладки/расширений.
    _binding_counter = 0

    # -------------------------------------------------------------
    # Инициализация
    # -------------------------------------------------------------
    def __init__(
        self,
        albedo: tuple[float, float, float, float] = (1.0, 1.0, 1.0, 1.0),
        metallic: float = 0.0,
        roughness: float = 0.5,
        ao: float = 1.0,
        emissive: tuple[float, float, float] = (0.0, 0.0, 0.0),
        albedo_map: str | None = None,
        normal_map: str | None = None,
        metallic_map: str | None = None,
        roughness_map: str | None = None,
        ao_map: str | None = None,
        emissive_map: str | None = None,
    ) -> None:
        # ---------------------------------------------------------
        # 0️⃣  Уникальный id (не используется в текущей версии)
        # ---------------------------------------------------------
        self.binding_point = PBRMaterial._binding_counter
        PBRMaterial._binding_counter += 1

        # ---------------------------------------------------------
        # 1️⃣  Параметры материала (записываются в constant‑buffer
        #     только в том случае, если шейдер их использует)
        # ---------------------------------------------------------
        self._cb_data = np.array(
            list(albedo)                     # 4 float – albedo
            + [metallic, roughness, ao]      # 3 float
            + list(emissive)                 # 3 float – emissive
            + [0.0, 0.0, 0.0],               # 3 pad‑float’а
            dtype=np.float32,
        ).tobytes()                           # 48 байт, но сейчас не используется

        # ---------------------------------------------------------
        # 2️⃣  Путь к пользовательским картам (загружаются «лениво»)
        # ---------------------------------------------------------
        self._texture_paths: dict[str, str] = {}
        if albedo_map:
            self._texture_paths["albedo"] = albedo_map
        if normal_map:
            self._texture_paths["normal"] = normal_map
        if metallic_map:
            self._texture_paths["metallic"] = metallic_map
        if roughness_map:
            self._texture_paths["roughness"] = roughness_map
        if ao_map:
            self._texture_paths["ao"] = ao_map
        if emissive_map:
            self._texture_paths["emissive"] = emissive_map

        # После загрузки в `self.textures` будет храниться реальная
        # `DX12Texture`‑обёртка, возвращаемая `load_texture`.
        self.textures: dict[str, any] = {}

    # -------------------------------------------------------------
    # Внутренний помощник – загрузка всех отложенных карт
    # -------------------------------------------------------------
    def _ensure_textures(self, backend: DX12Backend) -> None:
        """
        Если карта ещё не загружена – вызываем `load_texture`,
        сохраняем полученный объект в `self.textures`.
        """
        for name, path in self._texture_paths.items():
            if name in self.textures:
                continue          # уже загружена

            try:
                tex = load_texture(path, backend)
                logger.debug(f"[Material] Loaded texture '{name}' from {path}")
            except Exception as exc:
                # При любой ошибке – создаём простую чёрную 1×1‑текстуру,
                # чтобы шейдер не падал.
                logger.error(f"[Material] Failed to load texture '{path}': {exc}")
                tex = backend.create_texture(
                    data=b"\x00\x00\x00\x00", w=1, h=1, fmt="RGBA8"
                )
            self.textures[name] = tex

    # -------------------------------------------------------------
    # Привязка материала к пайплайну
    # -------------------------------------------------------------
    def bind(self, backend: DX12Backend) -> None:
        """
        1️⃣  Гарантируем, что все карты загружены.
        2️⃣  Выбираем *первую* из загруженных карт (обычно albedo)
            и создаём SRV‑дескриптор в `cbv_srv_uav_heap`.
        3️⃣  Привязываем SRV к slot 1 (в корневой подписи он идёт
            сразу после CBV).
        """
        # -----------------------------------------------------------------
        # 0️⃣  Убедимся, что карта(и) находятся в виде DX12‑texture‑объекта
        # -----------------------------------------------------------------
        self._ensure_textures(backend)

        # -----------------------------------------------------------------
        # 1️⃣  Если пользователь не указал ни одной карты – ничего не делаем.
        #     В `ForwardRenderer` в момент инициализации уже создана
        #     «белая placeholder‑текстура» и привязана к slot 1, так что
        #     оставляем её.
        # -----------------------------------------------------------------
        if not self.textures:
            return

        # -----------------------------------------------------------------
        # 2️⃣  Берём первую (обычно albedo) текстуру.
        # -----------------------------------------------------------------
        tex = next(iter(self.textures.values()))

        # -----------------------------------------------------------------
        # 3️⃣  Выделяем дескриптор в heap‑е, создаём SRV и привязываем.
        # -----------------------------------------------------------------
        srv_idx = backend.cbv_srv_uav_heap.next_free()
        cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(srv_idx)
        backend.create_shader_resource_view(tex, cpu_handle)

        # GPU‑handle, который передаём в root‑signature (slot 1)
        srv_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(srv_idx)
        backend.set_root_descriptor_table(1, srv_gpu)

        # -------------------------------------------------------------
        # (Если в будущем понадобится несколько текстур – просто
        #  добавить их в `self.textures` и привязать к другим слотам.)
        # -------------------------------------------------------------