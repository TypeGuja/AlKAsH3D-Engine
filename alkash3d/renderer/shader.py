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
    # простая таблица смещений внутри constant‑buffer
    _MAT_OFFSETS = {
        "uView":   0,
        "uProj":  64,
        "uModel": 128,
        # пользовательские uniform‑ы можно добавить, указав их смещение
        # (например, "uTint": 192)
    }
    _CB_SIZE = 192  # 3 * 64 байт (по три матрицы 4×4)

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
        self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)

        # один CBV‑дескриптор для constant‑buffer
        idx = backend.cbv_srv_uav_heap.next_free()
        cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
        backend.create_shader_resource_view(self._frame_cb, cpu_handle)
        self._frame_cb_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)

        # локальная копия данных (позволяет менять только нужные части)
        self._frame_data = bytearray(self._CB_SIZE)

        # Сохранить времена изменения файлов для hot‑reload
        self._vs_mtime = os.path.getmtime(vertex_path)
        self._ps_mtime = os.path.getmtime(fragment_path)

    # -----------------------------------------------------------------
    def use(self) -> None:
        """Привязать PSO к текущему командному списку."""
        self.backend.set_graphics_pipeline(self.pso)

    # -----------------------------------------------------------------
    def set_uniform_mat4(self, name: str, mat) -> None:
        """Записать 4×4‑матрицу в constant‑buffer."""
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown mat4 uniform: {name}")
            return
        offset = self._MAT_OFFSETS[name]
        arr = np.asarray(mat, dtype=np.float32).reshape(16)
        self._frame_data[offset: offset + 64] = arr.tobytes()
        self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
        self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)

    # -----------------------------------------------------------------
    def set_uniform_vec3(self, name: str, vec) -> None:
        """Записать vec3 (12 байт) в constant‑buffer."""
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown vec3 uniform: {name}")
            return
        offset = self._MAT_OFFSETS[name]
        arr = np.asarray(vec, dtype=np.float32).reshape(3)
        self._frame_data[offset: offset + 12] = arr.tobytes()
        self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
        self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)

    # -----------------------------------------------------------------
    def set_uniform_int(self, name: str, value: int) -> None:
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown int uniform: {name}")
            return
        offset = self._MAT_OFFSETS[name]
        self._frame_data[offset: offset + 4] = struct.pack("<i", int(value))
        self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
        self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)

    # -----------------------------------------------------------------
    def set_uniform_float(self, name: str, value: float) -> None:
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown float uniform: {name}")
            return
        offset = self._MAT_OFFSETS[name]
        self._frame_data[offset: offset + 4] = struct.pack("<f", float(value))
        self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
        self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)

    # -----------------------------------------------------------------
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