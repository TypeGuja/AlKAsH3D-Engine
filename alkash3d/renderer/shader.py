# alkash3d/renderer/shader.py
# -*- coding: utf-8 -*-

import os
import numpy as np
from typing import Any, Tuple

from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend


class Shader:
    _CB_SIZE = 256

    def __init__(
            self,
            vertex_path: str,
            fragment_path: str,
            backend: DX12Backend,
            vs_entry: str = "VSMain",
            ps_entry: str = "PSMain",
    ):
        self.backend = backend
        self.vertex_path = vertex_path
        self.fragment_path = fragment_path

        logger.info(
            f"[Shader] Loading VS={os.path.basename(vertex_path)}  "
            f"PS={os.path.basename(fragment_path)}"
        )

        self.vs_blob = backend.compile_shader("vs", vertex_path, entry_point=vs_entry)
        self.ps_blob = backend.compile_shader("ps", fragment_path, entry_point=ps_entry)

        self.pso = backend.create_graphics_ps(self.vs_blob, self.ps_blob)
        logger.info(f"[Shader] PSO created: 0x{self.pso:x}")

        self._cb_slot = backend.cbv_srv_uav_heap.next_free()
        self._tex_slot = backend.cbv_srv_uav_heap.next_free()

        logger.info(f"[Shader] CB slot: {self._cb_slot}, Texture slot: {self._tex_slot}")

        # Создаём constant buffer
        self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)

        if not self._frame_cb or not getattr(self._frame_cb, "value", 0):
            raise RuntimeError("Failed to create constant buffer")

        # Создаём CBV (Constant Buffer View) в heap
        cb_cpu = backend.cbv_srv_uav_heap.get_cpu_handle(self._cb_slot)
        if not backend.create_constant_buffer_view(self._frame_cb, cb_cpu):
            raise RuntimeError("Failed to create constant buffer view")

        # ✅ ВАЖНО: для root descriptor table нужен GPU handle!
        self._descriptor_table_handle = backend.cbv_srv_uav_heap.get_gpu_handle(self._cb_slot)
        logger.info(f"[Shader] Descriptor table GPU handle: 0x{self._descriptor_table_handle:X}")

        self._frame_data = bytearray(self._CB_SIZE)
        self._dirty = False

    def use(self) -> bool:
        if not self.pso:
            logger.error("[Shader] No valid PSO")
            return False
        try:
            return self.backend.set_graphics_pipeline(self.pso)
        except Exception as e:
            logger.error(f"[Shader] use error: {e}")
            return False

    def _write_to_cb(self, offset: int, data_bytes: bytes) -> None:
        end = offset + len(data_bytes)
        if end > self._CB_SIZE:
            logger.warning(f"Data too large: {end} > {self._CB_SIZE}")
            return
        if self._frame_data[offset:end] != data_bytes:
            self._frame_data[offset:end] = data_bytes
            self._dirty = True

    def set_uniform_mat4(self, name: str, mat: Any) -> None:
        offsets = {"uView": 0, "uProj": 64, "uModel": 128}
        offset = offsets.get(name)
        if offset is None:
            logger.warning(f"[Shader] Unknown mat4 uniform: {name}")
            return
        try:
            arr = np.asarray(mat, dtype=np.float32).reshape(16)
            self._write_to_cb(offset, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] set_uniform_mat4({name}) error: {e}")

    def set_uniform_vec4(self, name: str, value: Tuple[float, float, float, float]) -> None:
        offsets = {"uTint": 192}
        offset = offsets.get(name)
        if offset is None:
            logger.warning(f"[Shader] Unknown vec4 uniform: {name}")
            return
        try:
            arr = np.array(value, dtype=np.float32)
            self._write_to_cb(offset, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] set_uniform_vec4({name}) error: {e}")

    def flush(self) -> None:
        """Отправляет данные в GPU."""
        if not self._dirty:
            logger.debug("[Shader] No dirty data, skipping flush")
            return

        logger.info(f"[Shader] Flushing {self._CB_SIZE} bytes, dirty={self._dirty}")

        # Обновляем constant buffer
        if not self.backend.update_buffer(self._frame_cb, bytes(self._frame_data)):
            logger.error("[Shader] Failed to update constant buffer")
            return

        # Проверяем, что command list существует и не закрыт
        # Устанавливаем descriptor table (root parameter 0 = CBV)
        logger.info(f"[Shader] Setting root descriptor table with handle 0x{self._descriptor_table_handle:X}")

        if not self.backend.set_root_descriptor_table(0, self._descriptor_table_handle):
            logger.error("[Shader] Failed to set descriptor table")
            return

        self._dirty = False
        logger.debug("[Shader] Flush completed")

    @property
    def cb_slot(self) -> int:
        return self._cb_slot

    @property
    def tex_slot(self) -> int:
        return self._tex_slot

    def __repr__(self) -> str:
        return f"Shader(vs={os.path.basename(self.vertex_path)}, ps={os.path.basename(self.fragment_path)}, pso=0x{self.pso:x})"