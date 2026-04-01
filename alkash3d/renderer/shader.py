# alkash3d/renderer/shader.py
# -*- coding: utf-8 -*-

from __future__ import annotations
import ctypes
import traceback
from typing import Any, Optional
from alkash3d.utils import logger


class Shader:
    """Управляет шейдерной программой и дескрипторной таблицей."""

    def __init__(self, backend, vs_path: str, ps_path: str):
        """
        Инициализирует шейдер.

        Args:
            backend: Графический бэкенд
            vs_path: Путь к вершинному шейдеру
            ps_path: Путь к пиксельному шейдеру
        """
        self.backend = backend
        self.vs_path = vs_path
        self.ps_path = ps_path
        self.pso = None
        self.heap = None
        self.cb_buffer = None
        self._descriptor_table_handle = 0
        self._descriptor_table_set = False

    def compile(self) -> bool:
        """Компилирует шейдеры и создаёт PSO."""
        try:
            logger.info(f"[Shader] Loading VS={self.vs_path} PS={self.ps_path}")

            # Проверяем существование файлов
            if not os.path.exists(self.vs_path):
                logger.error(f"[Shader] Vertex shader not found: {self.vs_path}")
                return False
            if not os.path.exists(self.ps_path):
                logger.error(f"[Shader] Pixel shader not found: {self.ps_path}")
                return False

            # Компилируем шейдеры
            vs_blob = self.backend.compile_shader("vs", self.vs_path, "VSMain")
            ps_blob = self.backend.compile_shader("ps", self.ps_path, "PSMain")

            if not vs_blob or not ps_blob:
                logger.error("[Shader] Failed to compile shaders")
                return False

            # Создаём PSO
            self.pso = self.backend.create_graphics_ps(vs_blob, ps_blob)

            if not self.pso:
                logger.error("[Shader] Failed to create PSO")
                return False

            logger.info(f"[Shader] PSO created: {hex(self.pso)}")

            # Создаём дескрипторную кучу
            self.heap = self.backend.create_descriptor_heap(
                num_descriptors=1024,
                heap_type="cbv_srv_uav",
                shader_visible=True
            )

            if not self.heap:
                logger.error("[Shader] Failed to create descriptor heap")
                return False

            # Создаём константный буфер
            self._setup_constant_buffer()

            # Создаём текстуру-заглушку
            self._setup_texture()

            # Получаем GPU handle для дескрипторной таблицы
            self._descriptor_table_handle = self.heap.get_gpu_handle(0)
            logger.info(f"[Shader] Descriptor table GPU handle: 0x{self._descriptor_table_handle:X}")

            return True

        except Exception as e:
            logger.error(f"[Shader] Compilation error: {e}")
            traceback.print_exc()
            return False

    def _setup_constant_buffer(self):
        """Создаёт константный буфер и view."""
        try:
            # Создаём константный буфер (256 байт)
            cb_data = bytes(256)
            self.cb_buffer = self.backend.create_constant_buffer(cb_data)

            if self.cb_buffer:
                # Создаём CBV в куче (слот 0)
                cpu_handle = self.heap.get_cpu_handle(0)
                if not self.backend.create_constant_buffer_view(self.cb_buffer, cpu_handle):
                    logger.error("[Shader] Failed to create constant buffer view")
                else:
                    logger.info("[Shader] Constant buffer view created at slot 0")
        except Exception as e:
            logger.error(f"[Shader] Error setting up constant buffer: {e}")

    def _setup_texture(self):
        """Создаёт белую текстуру-заглушку."""
        try:
            # Создаём текстуру 1x1 белого цвета
            white_data = bytes([255, 255, 255, 255])  # RGBA
            texture = self.backend.create_texture(white_data, 1, 1, "RGBA8")

            if texture:
                # Создаём SRV в куче (слот 1)
                cpu_handle = self.heap.get_cpu_handle(1)
                if not self.backend.create_shader_resource_view(texture, cpu_handle):
                    logger.error("[Shader] Failed to create texture SRV")
                else:
                    logger.info("[Shader] White texture SRV created at slot 1")
        except Exception as e:
            logger.error(f"[Shader] Error setting up texture: {e}")

    def use(self) -> bool:
        """Активирует шейдерную программу."""
        if not self.pso:
            logger.error("[Shader] No PSO available")
            return False

        logger.debug("[Shader] use() called")

        # 1. Начинаем кадр
        if not self.backend.begin_frame():
            logger.error("[Shader] begin_frame failed")
            return False

        # 2. Устанавливаем PSO
        if not self.backend.set_graphics_pipeline(self.pso):
            logger.error("[Shader] set_graphics_pipeline failed")
            return False

        # 3. Устанавливаем дескрипторные кучи
        if not self.backend.set_descriptor_heaps([self.heap.heap_ptr]):
            logger.error("[Shader] set_descriptor_heaps failed")
            return False

        # 4. Устанавливаем root descriptor table
        gpu_handle = self._descriptor_table_handle

        # Проверяем валидность handle
        if gpu_handle == 0:
            logger.error("[Shader] GPU handle is 0 - cannot set descriptor table")
            # Пробуем использовать CPU handle как fallback
            gpu_handle = self.heap.get_cpu_handle(0)
            logger.warning(f"[Shader] Using CPU handle as fallback: 0x{gpu_handle:X}")

        try:
            result = self.backend.set_root_descriptor_table(0, gpu_handle)
            if not result:
                logger.error("[Shader] set_root_descriptor_table failed")
                return False
        except Exception as e:
            logger.error(f"[Shader] set_root_descriptor_table exception: {e}")
            return False

        self._descriptor_table_set = True
        logger.debug("[Shader] Successfully activated")
        return True

    def set_constant_buffer_data(self, data: bytes) -> bool:
        """Обновляет данные в константном буфере."""
        if not self.cb_buffer:
            logger.error("[Shader] No constant buffer")
            return False

        return self.backend.update_buffer(self.cb_buffer, data)

    def cleanup(self):
        """Освобождает ресурсы."""
        if self.cb_buffer:
            try:
                self.backend.release_resource(self.cb_buffer)
            except Exception as e:
                logger.error(f"[Shader] Error releasing constant buffer: {e}")
            self.cb_buffer = None

        if self.heap:
            try:
                self.backend.release_resource(self.heap.heap_ptr)
            except Exception as e:
                logger.error(f"[Shader] Error releasing descriptor heap: {e}")
            self.heap = None

        self._descriptor_table_set = False
        logger.debug("[Shader] Cleaned up")

    def __del__(self):
        """Деструктор."""
        self.cleanup()


# Добавляем импорт os, который используется в методе compile
import os