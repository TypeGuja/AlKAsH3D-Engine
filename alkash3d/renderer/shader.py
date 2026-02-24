# alkash3d/renderer/shader.py - ИСПРАВЛЕННАЯ ВЕРСИЯ

import os
import numpy as np
import ctypes
from typing import Optional, Any
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

    # alkash3d/renderer/shader.py

    def __init__(self, vertex_path: str, fragment_path: str, backend: DX12Backend):
        self.backend = backend
        self.vertex_path = vertex_path
        self.fragment_path = fragment_path

        logger.info(f"[Shader] Initialising shader program: {os.path.basename(vertex_path)}")

        # Проверяем существование файлов
        if not os.path.exists(vertex_path):
            logger.error(f"[Shader] Vertex shader not found: {vertex_path}")
            self.vs_blob = 0
            self.pso = None
            return

        if not os.path.exists(fragment_path):
            logger.error(f"[Shader] Fragment shader not found: {fragment_path}")
            self.ps_blob = 0
            self.pso = None
            return

        # Компиляция
        self.vs_blob = backend.compile_shader("vs", vertex_path)
        self.ps_blob = backend.compile_shader("ps", fragment_path)

        # ИСПРАВЛЕНИЕ: Создаем правильные указатели
        vs_ptr = None
        ps_ptr = None

        if self.vs_blob and self.vs_blob != 0x12345678:
            # Убеждаемся что это правильный указатель
            if isinstance(self.vs_blob, int):
                vs_ptr = ctypes.c_void_p(self.vs_blob)
            elif hasattr(self.vs_blob, 'value'):
                vs_ptr = ctypes.c_void_p(self.vs_blob.value)
            else:
                vs_ptr = ctypes.c_void_p(int(self.vs_blob))

        if self.ps_blob and self.ps_blob != 0x12345678:
            if isinstance(self.ps_blob, int):
                ps_ptr = ctypes.c_void_p(self.ps_blob)
            elif hasattr(self.ps_blob, 'value'):
                ps_ptr = ctypes.c_void_p(self.ps_blob.value)
            else:
                ps_ptr = ctypes.c_void_p(int(self.ps_blob))

        # PSO - ИСПРАВЛЕНИЕ: Проверяем что указатели валидны
        self.pso = None

        if vs_ptr and ps_ptr and vs_ptr.value and ps_ptr.value:
            try:
                logger.debug(f"[Shader] Creating PSO with vs={hex(vs_ptr.value)}, ps={hex(ps_ptr.value)}")
                pso_result = backend.create_graphics_ps(vs_ptr, ps_ptr)

                if pso_result and hasattr(pso_result, 'value') and pso_result.value:
                    self.pso = pso_result.value
                    logger.info(f"[Shader] PSO created successfully: {hex(self.pso)}")
                elif isinstance(pso_result, int) and pso_result and pso_result != 0x87654321:
                    self.pso = pso_result
                    logger.info(f"[Shader] PSO created successfully: {hex(self.pso)}")
                else:
                    logger.error("[Shader] PSO creation returned invalid value")
                    self.pso = None
            except Exception as e:
                logger.error(f"[Shader] PSO creation failed: {e}")
                import traceback
                traceback.print_exc()
                self.pso = None
        else:
            logger.error(
                f"[Shader] Cannot create PSO - invalid shader blobs: vs={vs_ptr.value if vs_ptr else None}, ps={ps_ptr.value if ps_ptr else None}")
            self.pso = None

        # Constant buffer
        try:
            self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)
            self._frame_cb_gpu = 0

            if (self._frame_cb and hasattr(self._frame_cb, 'value') and
                    self._frame_cb.value and self._frame_cb.value != 0xDEADBEEF and
                    hasattr(backend, 'cbv_srv_uav_heap') and backend.cbv_srv_uav_heap):

                idx = backend.cbv_srv_uav_heap.next_free()
                cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
                if backend.create_constant_buffer_view(self._frame_cb, cpu_handle):
                    self._frame_cb_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)
                    logger.debug(f"[Shader] CBV created at GPU handle: {hex(self._frame_cb_gpu)}")
        except Exception as e:
            logger.error(f"[Shader] Failed to create constant buffer: {e}")
            self._frame_cb = None
            self._frame_cb_gpu = 0

        self._frame_data = bytearray(self._CB_SIZE)
        self._dirty = False

    def use(self) -> bool:
        """Активировать шейдер."""
        if self.pso and self.pso != 0x87654321:
            try:
                # ИСПРАВЛЕНИЕ: Передаем значение PSO как есть
                result = self.backend.set_graphics_pipeline(self.pso)
                return result
            except Exception as e:
                logger.error(f"[Shader] Failed to set pipeline: {e}")
                return False
        else:
            logger.warning("[Shader] No valid PSO to use")
            return False

    def use(self) -> bool:
        """Активировать шейдер."""
        if self.pso and self.pso != 0x87654321:
            try:
                result = self.backend.set_graphics_pipeline(self.pso)
                return result
            except Exception as e:
                logger.error(f"[Shader] Failed to set pipeline: {e}")
                return False
        else:
            logger.warning("[Shader] No valid PSO to use")
            return False

    def _write_to_cb(self, name: str, data_bytes: bytes) -> None:
        if name not in self._MAT_OFFSETS:
            return
        offset = self._MAT_OFFSETS[name]
        old_data = self._frame_data[offset:offset + len(data_bytes)]
        if old_data != data_bytes:
            self._frame_data[offset:offset + len(data_bytes)] = data_bytes
            self._dirty = True

    def _flush_cb(self) -> None:
        if self._dirty and self._frame_cb and self._frame_cb_gpu:
            try:
                self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
                self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)
                self._dirty = False
            except Exception as e:
                logger.error(f"[Shader] Failed to flush CB: {e}")

    def set_uniform_mat4(self, name: str, mat: Any) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            arr = np.asarray(mat, dtype=np.float32).reshape(16)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] Error setting mat4 {name}: {e}")

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
            logger.error(f"[Shader] Error setting vec3 {name}: {e}")

    def set_uniform_float(self, name: str, value: float) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            arr = np.array([value], dtype=np.float32)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] Error setting float {name}: {e}")

    def set_uniform_int(self, name: str, value: int) -> None:
        if name not in self._MAT_OFFSETS:
            return
        try:
            arr = np.array([value], dtype=np.int32)
            self._write_to_cb(name, arr.tobytes())
        except Exception as e:
            logger.error(f"[Shader] Error setting int {name}: {e}")

    def flush(self) -> None:
        self._flush_cb()