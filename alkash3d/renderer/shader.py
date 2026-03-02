# alkash3d/renderer/shader.py
# -*- coding: utf-8 -*-
"""
Обёртка над VS/PS‑blob‑ами и готовым PSO.
Поддерживает произвольные имена entry‑point‑ов (по‑умолчанию VSMain/PSMain).
"""

import ctypes
import os
import numpy as np
from typing import Optional, Any

from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend


class Shader:
    """Обёртка над парой шейдер‑blob‑ов и PSO.

    По‑умолчанию ищет функции VSMain / PSMain.
    Если ваши HLSL‑файлы используют entry‑point «main», передайте
    явные имена через параметры `vs_entry` и `ps_entry`.
    """

    # -----------------------------------------------------------------
    # Смещения в constant‑buffer (по 32 байта на переменную)
    # -----------------------------------------------------------------
    _MAT_OFFSETS = {
        "uView": 0,
        "uProj": 64,
        "uModel": 128,
        "uTint": 192,
        "uTime": 208,
        "uNumLights": 212,
    }

    _CB_SIZE = 256  # байт

    # -----------------------------------------------------------------
    def __init__(
        self,
        vertex_path: str,
        fragment_path: str,
        backend: DX12Backend,
        vs_entry: str = "VSMain",   # <‑‑ по‑умолчанию старые имена
        ps_entry: str = "PSMain",
    ):
        """
        Parameters
        ----------
        vertex_path, fragment_path : str
            Полные пути к HLSL‑файлам.
        backend : DX12Backend
            Бэкенд, через который будет выполнена компиляция.
        vs_entry, ps_entry : str, optional
            Имена функций‑входов в HLSL‑файлах.
            По‑умолчанию – VSMain / PSMain (совместимо с оригинальными
            шейдерами).  Если ваши файлы используют entry‑point «main»,
            передайте `vs_entry="main"` и `ps_entry="main"`.
        """
        self.backend = backend
        self.vertex_path = vertex_path
        self.fragment_path = fragment_path

        logger.info(
            f"[Shader] Loading VS={os.path.basename(vertex_path)}  "
            f"PS={os.path.basename(fragment_path)}"
        )

        # -------------------------------------------------------------
        # Компилируем шейдеры (DX12Backend.compile_shader умеет принимать
        # optional entry_point)
        # -------------------------------------------------------------
        self.vs_blob = backend.compile_shader(
            "vs", vertex_path, entry_point=vs_entry
        )
        self.ps_blob = backend.compile_shader(
            "ps", fragment_path, entry_point=ps_entry
        )

        # -------------------------------------------------------------
        # Создаём PSO (если оба blob‑а получены)
        # -------------------------------------------------------------
        self.pso: Optional[int] = None
        if self.vs_blob and self.ps_blob:
            try:
                logger.debug(
                    f"[Shader] Creating PSO: vs=0x{self.vs_blob:x} ps=0x{self.ps_blob:x}"
                )
                pso_res = backend.create_graphics_ps(self.vs_blob, self.ps_blob)

                # `create_graphics_ps` может вернуть int или объект с .value
                if isinstance(pso_res, int) and pso_res:
                    self.pso = pso_res
                elif hasattr(pso_res, "value") and pso_res.value:
                    self.pso = pso_res.value
            except Exception as e:
                logger.error(f"[Shader] PSO creation failed: {e}")

        # -------------------------------------------------------------
        # Constant‑buffer (256 байт) + SRV‑дескриптер
        # -------------------------------------------------------------
        self._frame_cb = backend.create_constant_buffer(b"\x00" * self._CB_SIZE)
        self._frame_cb_gpu = 0

        if self._frame_cb and getattr(self._frame_cb, "value", 0):
            idx = backend.cbv_srv_uav_heap.next_free()
            cpu = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
            if backend.create_constant_buffer_view(self._frame_cb, cpu):
                self._frame_cb_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)

        # Хранилище данных, которые будем писать в CB
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
    # ----------  Constant‑buffer handling  ----------
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
    # ----------  Uniform‑setters  ----------
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
        """Синхронно отослать накопленные uniform‑ы в GPU."""
        self._flush_cb()
