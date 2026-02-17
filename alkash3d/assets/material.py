# -*- coding: utf-8 -*-
"""
PBR‑материал – хранит параметры (albedo, metallic, roughness, ao,
emissive) и ссылки на DX12‑текстуры.

* Текстуры **загружаются** (по‑запросу) в методе `_ensure_textures`.
* Для материала **не создаётся свой CBV** – все матрицы передаются через
  один constant‑buffer, который создаёт `Shader` (`Shader._frame_cb`).
* После загрузки текстуры создаётся **SRV‑дескриптор** в heap‑е
  `cbv_srv_uav_heap`, а затем вызывается
  `backend.set_root_descriptor_table(1, gpu_handle)`.  Слот 1
  соответствует **SRV** в корневой подписи (CBV – slot 0,
  SRV – slot 1).
"""

from __future__ import annotations

import numpy as np
from alkash3d.utils import logger
from alkash3d.utils.texture_loader import load_texture
from alkash3d.graphics.dx12_backend import DX12Backend, DX12Texture


class PBRMaterial:
    """Хранит параметры PBR‑материала и ссылки на DX12‑текстуры."""
    _binding_counter = 0

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
        self.binding_point = PBRMaterial._binding_counter
        PBRMaterial._binding_counter += 1

        # ---------------------------------------------------------
        self._cb_data = np.array(
            list(albedo) + [metallic, roughness, ao] + list(emissive) + [0.0, 0.0, 0.0],
            dtype=np.float32,
        ).tobytes()      # пока не используется

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

        self.textures: dict[str, DX12Texture] = {}
        self._srv_index: int | None = None   # один SRV‑дескриптор для всех карт

    # -------------------------------------------------------------
    def _ensure_textures(self, backend: DX12Backend) -> None:
        """Загружаем отложенные текстуры и создаём один SRV‑дескриптор."""
        for name, path in self._texture_paths.items():
            if name in self.textures:
                continue
            try:
                tex = load_texture(path, backend)
                logger.debug(f"[Material] Loaded texture '{name}' from {path}")
            except Exception as exc:
                logger.error(f"[Material] Failed to load texture '{path}': {exc}")
                tex = backend.create_texture(
                    data=b"\x00\x00\x00\x00", w=1, h=1, fmt="RGBA8"
                )
            self.textures[name] = tex

        # Если SRV‑дескриптор ещё не создан – делаем его сейчас
        if self._srv_index is None and self.textures:
            self._srv_index = backend.cbv_srv_uav_heap.next_free()
            cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(self._srv_index)
            first_tex = next(iter(self.textures.values()))
            backend.create_shader_resource_view(first_tex, cpu_handle)

    # -------------------------------------------------------------
    def bind(self, backend: DX12Backend) -> None:
        """Привязать материал к пайплайну (устанавливаем SRV‑slot 1)."""
        self._ensure_textures(backend)

        if self._srv_index is None:
            # Если вообще нет текстур – оставляем привязанным placeholder‑текстуру,
            # которая обычно создаётся в ForwardRenderer.
            return

        gpu_handle = backend.cbv_srv_uav_heap.get_gpu_handle(self._srv_index)
        backend.set_root_descriptor_table(1, gpu_handle)
