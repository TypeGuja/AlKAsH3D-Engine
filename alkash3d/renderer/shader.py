# alkash3d/renderer/shader.py - ИСПРАВЛЕННАЯ ВЕРСИЯ

import os
import struct
import numpy as np
from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend


class Shader:
    """Обёртка над парой VS/PS‑blob‑ов и готовым PSO."""

    _MAT_OFFSETS = {
        "uView": 0,
        "uProj": 64,
        "uModel": 128,
        "uTint": 192,
        "uTime": 208,
        "uNumLights": 212,
    }

    _CB_SIZE = 256

    def __init__(self, vertex_path: str, fragment_path: str, backend: DX12Backend):
        self.backend = backend
        self.vertex_path = vertex_path
        self.fragment_path = fragment_path

        logger.info("[Shader] Initialising shader program")

        # Компиляция
        self.vs_blob = backend.compile_shader("vs", vertex_path)
        self.ps_blob = backend.compile_shader("ps", fragment_path)

        # PSO
        self.pso = backend.create_graphics_ps(self.vs_blob, self.ps_blob)

        # ===== ИСПРАВЛЕНИЕ =====
        # create_constant_buffer возвращает ТОЛЬКО буфер, не кортеж
        self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)

        # Создаем CBV отдельно
        idx = backend.cbv_srv_uav_heap.next_free()
        cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
        backend.create_constant_buffer_view(self._frame_cb, cpu_handle)
        self._frame_cb_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)
        # ======================

        self._frame_data = bytearray(self._CB_SIZE)
        self._dirty = False

    def use(self):
        self.backend.set_graphics_pipeline(self.pso)

    def _write_to_cb(self, name: str, data_bytes: bytes):
        if name not in self._MAT_OFFSETS:
            return
        offset = self._MAT_OFFSETS[name]
        self._frame_data[offset:offset + len(data_bytes)] = data_bytes
        self._dirty = True

    def _flush_cb(self):
        if self._dirty:
            self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
            self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)
            self._dirty = False

    def set_uniform_mat4(self, name: str, mat):
        if name not in self._MAT_OFFSETS:
            return
        arr = np.asarray(mat, dtype=np.float32).reshape(16)
        self._write_to_cb(name, arr.tobytes())

    def set_uniform_vec3(self, name: str, vec):
        if name not in self._MAT_OFFSETS:
            return
        if hasattr(vec, "as_np"):
            arr = np.asarray(vec.as_np(), dtype=np.float32).reshape(3)
        else:
            arr = np.asarray(vec, dtype=np.float32).reshape(3)
        self._write_to_cb(name, arr.tobytes())

    def flush(self):
        self._flush_cb()