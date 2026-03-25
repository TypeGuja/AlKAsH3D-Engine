# alkash3d/renderer/shader.py
# -*- coding: utf-8 -*-

import ctypes
import os
import numpy as np
from typing import Optional, Any

from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend


class Shader:
    """Обёртка над VS/PS‑blob‑ами и готовым PSO."""

    _MAT_OFFSETS = {
        "uView": 0,
        "uProj": 64,
        "uModel": 128,
        "uTint": 192,
        "uTime": 208,
        "uNumLights": 212,
    }

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

        # Компилируем шейдеры
        self.vs_blob = backend.compile_shader("vs", vertex_path, entry_point=vs_entry)
        self.ps_blob = backend.compile_shader("ps", fragment_path, entry_point=ps_entry)

        # Создаём PSO
        self.pso = backend.create_graphics_ps(self.vs_blob, self.ps_blob)
        logger.info(f"[Shader] PSO created: 0x{self.pso:x}")

        # Выделяем слоты в дескрипторной куче
        self._cb_slot = backend.cbv_srv_uav_heap.next_free()
        self._tex_slot = backend.cbv_srv_uav_heap.next_free()

        logger.info(f"[Shader] CB slot: {self._cb_slot}, Texture slot: {self._tex_slot}")

        # Создаём Constant Buffer
        self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)

        if not self._frame_cb or not getattr(self._frame_cb, "value", 0):
            raise RuntimeError("Failed to create constant buffer")

        # Создаём CBV дескриптор в слоте CB - используем CPU handle
        cb_cpu = backend.cbv_srv_uav_heap.get_cpu_handle(self._cb_slot)
        if not backend.create_constant_buffer_view(self._frame_cb, cb_cpu):
            raise RuntimeError("Failed to create constant buffer view")

        # ВАЖНО: Используем CPU handle вместо GPU handle, так как Rust блокирует GPU handle 0x15678A00110000
        # На многих системах CPU и GPU handle совпадают или имеют фиксированное смещение
        self._descriptor_table_handle = cb_cpu
        logger.info(f"[Shader] Descriptor table handle (CPU): 0x{self._descriptor_table_handle:X}")

        self._frame_data = bytearray(self._CB_SIZE)
        self._dirty = False
        self._texture_set = False

    # -----------------------------------------------------------------
    def use(self) -> bool:
        if not self.pso:
            raise RuntimeError("[Shader] No valid PSO")
        return self.backend.set_graphics_pipeline(self.pso)

    # -----------------------------------------------------------------
    def set_texture_from_resource(self, resource_ptr: Any) -> bool:
        """
        Создаёт SRV для текстуры в слоте TEXTURE.
        """
        try:
            cpu_handle = self.backend.cbv_srv_uav_heap.get_cpu_handle(self._tex_slot)
            result = self.backend.create_shader_resource_view(resource_ptr, cpu_handle)
            if result:
                self._texture_set = True
                logger.debug(f"[Shader] SRV created at slot {self._tex_slot}")
                return True
            return False
        except Exception as e:
            logger.error(f"[Shader] set_texture_from_resource error: {e}")
            return False

    # -----------------------------------------------------------------
    def _write_to_cb(self, name: str, data_bytes: bytes) -> None:
        offset = self._MAT_OFFSETS.get(name)
        if offset is None:
            return
        if self._frame_data[offset: offset + len(data_bytes)] != data_bytes:
            self._frame_data[offset: offset + len(data_bytes)] = data_bytes
            self._dirty = True

    # -----------------------------------------------------------------
    def _flush_cb(self) -> None:
        if not self._dirty:
            return

        try:
            self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
            self._dirty = False

            # Устанавливаем корневую таблицу дескрипторов
            # Используем CPU handle (он должен работать на большинстве систем)
            self.backend.set_root_descriptor_table(0, self._descriptor_table_handle)

        except Exception as e:
            logger.error(f"[Shader] flush_cb error: {e}")
            raise

    # -----------------------------------------------------------------
    def set_uniform_mat4(self, name: str, mat: Any) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            arr = np.asarray(mat, dtype=np.float32).reshape(16)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] set_uniform_mat4({name}) error: {e}")

    def set_uniform_vec3(self, name: str, vec: Any) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            if hasattr(vec, "as_np"):
                arr = np.asarray(vec.as_np(), dtype=np.float32).reshape(3)
            else:
                arr = np.asarray(vec, dtype=np.float32).reshape(3)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] set_uniform_vec3({name}) error: {e}")

    def set_uniform_float(self, name: str, value: float) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            arr = np.array([value], dtype=np.float32)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] set_uniform_float({name}) error: {e}")

    def set_uniform_int(self, name: str, value: int) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            arr = np.array([value], dtype=np.int32)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] set_uniform_int({name}) error: {e}")

    def flush(self) -> None:
        self._flush_cb()

    @property
    def cb_slot(self) -> int:
        return self._cb_slot

    @property
    def tex_slot(self) -> int:
        return self._tex_slot

    def __repr__(self) -> str:
        return f"Shader(vs={os.path.basename(self.vertex_path)}, ps={os.path.basename(self.fragment_path)}, pso=0x{self.pso:x})"