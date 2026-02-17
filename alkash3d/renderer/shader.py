# alkash3d/renderer/shader.py
# -*- coding: utf-8 -*-
"""
Простейший менеджер HLSL‑шейдеров для DirectX 12.
* Компилирует VS/PS через DX12‑бекенд.
* Создаёт один constant‑buffer, в который записываются матрицы и
  пользовательские uniform‑ы.
"""

import os
import struct
import numpy as np
from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend


class Shader:
    """Обёртка над парой VS/PS‑blob‑ов и готовым PSO."""
    # смещения внутри constant‑buffer (по 64 байта для 4×4 матриц)
    _MAT_OFFSETS = {
        "uView":   0,
        "uProj":   64,
        "uModel": 128,
        "uTint": 192,                 # vec3 (12 байт) – сразу после трёх матриц
    }

    # 3 матрицы (3*64) + место для vec3 (12) → 204, выравниваем до 256 байт
    _CB_SIZE = 256

    def __init__(self, backend: DX12Backend, vertex_path: str, fragment_path: str):
        self.backend = backend
        self.vertex_path = vertex_path
        self.fragment_path = fragment_path

        logger.info("[Shader] Initialising shader program")
        logger.debug(f"[Shader] VS: {vertex_path}")
        logger.debug(f"[Shader] PS: {fragment_path}")

        # ---------- компиляция ----------
        self.vs_blob = backend.compile_shader("vs", vertex_path)
        if not self.vs_blob:
            raise RuntimeError(f"Failed to compile vertex shader: {vertex_path}")

        self.ps_blob = backend.compile_shader("ps", fragment_path)
        if not self.ps_blob:
            raise RuntimeError(f"Failed to compile fragment shader: {fragment_path}")

        # ---------- PSO ----------
        self.pso = backend.create_graphics_ps(self.vs_blob, self.ps_blob)
        if not self.pso:
            raise RuntimeError("Failed to create graphics pipeline")

        # ---------- constant‑buffer ----------
        # backend.create_constant_buffer теперь возвращает (buffer, gpu_handle)
        self._frame_cb, self._frame_cb_gpu = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)

        # локальная копия данных (может менять только нужные части)
        self._frame_data = bytearray(self._CB_SIZE)

        # запоминаем времена изменения файлов (для hot‑reload)
        self._vs_mtime = os.path.getmtime(vertex_path)
        self._ps_mtime = os.path.getmtime(fragment_path)

    # -------------------------------------------------------------
    def use(self) -> None:
        """Привязать PSO к текущему командному списку."""
        self.backend.set_graphics_pipeline(self.pso)

    # -------------------------------------------------------------
    def _write_to_cb(self, name: str, data_bytes: bytes) -> None:
        """Общий помощник – записывает «data_bytes» в constant‑buffer."""
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown uniform: {name}")
            return
        offset = self._MAT_OFFSETS[name]
        self._frame_data[offset : offset + len(data_bytes)] = data_bytes
        self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
        self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)

    # -------------------------------------------------------------
    def set_uniform_mat4(self, name: str, mat) -> None:
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown mat4 uniform: {name}")
            return
        arr = np.asarray(mat, dtype=np.float32).reshape(16)
        self._write_to_cb(name, arr.tobytes())

    # -------------------------------------------------------------
    def set_uniform_vec3(self, name: str, vec) -> None:
        """Принимает либо обычный массив/список, либо объект Vec3."""
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown vec3 uniform: {name}")
            return

        # Если передан our‑Vec3 – берём массив через as_np()
        if hasattr(vec, "as_np"):
            arr = np.asarray(vec.as_np(), dtype=np.float32).reshape(3)
        else:
            arr = np.asarray(vec, dtype=np.float32).reshape(3)

        self._write_to_cb(name, arr.tobytes())

    # -------------------------------------------------------------
    def set_uniform_int(self, name: str, value: int) -> None:
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown int uniform: {name}")
            return
        self._write_to_cb(name, struct.pack("<i", int(value)))

    # -------------------------------------------------------------
    def set_uniform_float(self, name: str, value: float) -> None:
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown float uniform: {name}")
            return
        self._write_to_cb(name, struct.pack("<f", float(value)))

    # -------------------------------------------------------------
    def reload_if_needed(self) -> None:
        """Перекомпилировать шейдер, если файл изменился."""
        try:
            vs_mtime = os.path.getmtime(self.vertex_path)
            ps_mtime = os.path.getmtime(self.fragment_path)
        except OSError:
            return

        if vs_mtime != self._vs_mtime or ps_mtime != self._ps_mtime:
            logger.info("[Shader] Detected shader change – recompiling")
            self.vs_blob = self.backend.compile_shader("vs", self.vertex_path)
            self.ps_blob = self.backend.compile_shader("ps", self.fragment_path)
            self.pso = self.backend.create_graphics_ps(self.vs_blob, self.ps_blob)
            self._vs_mtime, self._ps_mtime = vs_mtime, ps_mtime