# alkash3d/renderer/shader.py
# -*- coding: utf-8 -*-
"""
Обёртка над VS/PS‑blob‑ами и готовым PSO.
Исправлена двойная реализация `use()`, добавлены проверки указателей.
"""

import os
import numpy as np
import ctypes
from typing import Optional, Any
from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend


class Shader:
    """Обёртка над парой шейдер‑blob‑ов и PSO."""

    # Смещения в constant‑buffer’е (32 байта на переменную)
    _MAT_OFFSETS = {
        "uView": 0,
        "uProj": 64,
        "uModel": 128,
        "uTint": 192,
        "uTime": 208,
        "uNumLights": 212,
    }

    _CB_SIZE = 256  # байт

    def __init__(self, vertex_path: str, fragment_path: str, backend: DX12Backend):
        self.backend = backend
        self.vertex_path = vertex_path
        self.fragment_path = fragment_path

        logger.info(f"[Shader] Initialising {os.path.basename(vertex_path)}")

        # -----------------------------------------------------------------
        # Проверяем существование файлов
        # -----------------------------------------------------------------
        if not os.path.isfile(vertex_path):
            logger.error(f"[Shader] Vertex shader not found: {vertex_path}")
            self.vs_blob = None
            self.pso = None
            return
        if not os.path.isfile(fragment_path):
            logger.error(f"[Shader] Fragment shader not found: {fragment_path}")
            self.ps_blob = None
            self.pso = None
            return

        # -----------------------------------------------------------------
        # Компилируем шейдеры через бекенд
        # -----------------------------------------------------------------
        self.vs_blob = backend.compile_shader("vs", vertex_path)
        self.ps_blob = backend.compile_shader("ps", fragment_path)

        # -----------------------------------------------------------------
        # Создаём PSO, если оба blob‑а валидны
        # -----------------------------------------------------------------
        self.pso = None
        if self.vs_blob and self.ps_blob:
            # Приводим к ctypes‑указателям
            vs_ptr = ctypes.c_void_p(self.vs_blob) if isinstance(self.vs_blob, int) else None
            ps_ptr = ctypes.c_void_p(self.ps_blob) if isinstance(self.ps_blob, int) else None

            if vs_ptr and ps_ptr and vs_ptr.value and ps_ptr.value:
                try:
                    logger.debug(f"[Shader] Creating PSO: vs={hex(vs_ptr.value)} ps={hex(ps_ptr.value)}")
                    pso_res = backend.create_graphics_ps(vs_ptr, ps_ptr)
                    if pso_res and getattr(pso_res, "value", 0):
                        self.pso = pso_res.value
                    elif isinstance(pso_res, int) and pso_res:
                        self.pso = pso_res
                except Exception as e:
                    logger.error(f"[Shader] PSO creation failed: {e}")

        # -----------------------------------------------------------------
        # Constant‑buffer (внутренний, 256 байт)
        # -----------------------------------------------------------------
        try:
            self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)
            self._frame_cb_gpu = 0
            if self._frame_cb and getattr(self._frame_cb, "value", 0):
                idx = backend.cbv_srv_uav_heap.next_free()
                cpu = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
                if backend.create_constant_buffer_view(self._frame_cb, cpu):
                    self._frame_cb_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)
        except Exception as e:
            logger.error(f"[Shader] Constant buffer init error: {e}")
            self._frame_cb = None
            self._frame_cb_gpu = 0

        self._frame_data = bytearray(self._CB_SIZE)
        self._dirty = False

    # -----------------------------------------------------------------
    def use(self) -> bool:
        """Активировать шейдер (установить PSO)."""
        if not self.pso:
            logger.warning("[Shader] No valid PSO – use() returns False")
            return False
        try:
            return self.backend.set_graphics_pipeline(self.pso)
        except Exception as e:
            logger.error(f"[Shader] set_graphics_pipeline failed: {e}")
            return False

    # -----------------------------------------------------------------
    # Запись данных в constant‑buffer
    # -----------------------------------------------------------------
    def _write_to_cb(self, name: str, data_bytes: bytes) -> None:
        offset = self._MAT_OFFSETS.get(name)
        if offset is None:
            return
        if self._frame_data[offset : offset + len(data_bytes)] != data_bytes:
            self._frame_data[offset : offset + len(data_bytes)] = data_bytes
            self._dirty = True

    def _flush_cb(self) -> None:
        if self._dirty and self._frame_cb and self._frame_cb_gpu:
            try:
                self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
                self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)
                self._dirty = False
            except Exception as e:
                logger.error(f"[Shader] flush_cb error: {e}")

    # -----------------------------------------------------------------
    # Утилиты для задания униформ
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

    # -----------------------------------------------------------------
    def flush(self) -> None:
        """Сбросить изменения в constant‑buffer (вызывается в конце кадра)."""
        self._flush_cb()
