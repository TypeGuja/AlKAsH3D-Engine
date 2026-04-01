# alkash3d/graphics/utils/descriptor_heap.py
# -*- coding: utf-8 -*-

from __future__ import annotations
import ctypes
from typing import Any
from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.utils import logger


class DescriptorHeap:
    HEAP_TYPE_RTV = 0
    HEAP_TYPE_DSV = 1
    HEAP_TYPE_CBV_SRV_UAV = 2

    def __init__(self,
                 device: Any,
                 num_descriptors: int,
                 heap_type: str = "cbv_srv_uav",
                 shader_visible: bool = True):

        self.device = device
        self.num_descriptors = int(num_descriptors)
        self.shader_visible = bool(shader_visible)
        self.heap_type_str = heap_type

        self.heap_type = {
            'rtv': self.HEAP_TYPE_RTV,
            'dsv': self.HEAP_TYPE_DSV,
            'cbv_srv_uav': self.HEAP_TYPE_CBV_SRV_UAV,
        }.get(heap_type.lower(), self.HEAP_TYPE_CBV_SRV_UAV)

        # Создаём кучу
        self.heap_ptr = dx.create_descriptor_heap(
            self.device,
            self.num_descriptors,
            self.heap_type,
            self.shader_visible
        )

        if not self.heap_ptr or not self.heap_ptr.value:
            raise RuntimeError("Failed to create descriptor heap")

        # Получаем CPU handle (всегда правильный)
        self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self.heap_ptr)
        if self.cpu_start == 0:
            raise RuntimeError("Failed to get CPU handle")

        # Получаем GPU handle
        self.gpu_start = 0
        self.use_cpu_fallback = False

        if self.shader_visible:
            raw_gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self.heap_ptr)

            # Проверка на битые handle от WARP драйвера
            BROKEN_HANDLES = [0, 0x15678A00110000, 0x25678A00120000, 0x35678A00130000, 0x45678A00140000]

            if raw_gpu_start in BROKEN_HANDLES:
                logger.warning(f"[DescriptorHeap] WARP/broken GPU handle detected: 0x{raw_gpu_start:X}")

                # Пытаемся пересоздать кучу без шейдерной видимости
                logger.info("[DescriptorHeap] Attempting to recreate heap without shader visibility")

                # Освобождаем старую кучу
                dx.release_resource(self.heap_ptr)

                # Создаём новую кучу без шейдерной видимости
                self.shader_visible = False
                self.heap_ptr = dx.create_descriptor_heap(
                    self.device,
                    self.num_descriptors,
                    self.heap_type,
                    False
                )

                if not self.heap_ptr or not self.heap_ptr.value:
                    raise RuntimeError("Failed to recreate descriptor heap without shader visibility")

                # Получаем новый CPU handle
                self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self.heap_ptr)
                if self.cpu_start == 0:
                    raise RuntimeError("Failed to get CPU handle for recreated heap")

                self.gpu_start = 0
                self.use_cpu_fallback = True
                logger.warning("[DescriptorHeap] Using CPU-only fallback mode (no GPU handles)")
            else:
                self.gpu_start = raw_gpu_start
                self.use_cpu_fallback = False
                logger.info(f"[DescriptorHeap] GPU start: 0x{self.gpu_start:X}")
        else:
            self.gpu_start = 0
            self.use_cpu_fallback = True

        # Размер дескриптора
        if self.heap_type == self.HEAP_TYPE_RTV:
            self.increment_size = dx.get_rtv_descriptor_size()
        elif self.heap_type == self.HEAP_TYPE_DSV:
            self.increment_size = dx.get_dsv_descriptor_size()
        else:
            self.increment_size = dx.get_rtv_descriptor_size()

        if self.increment_size == 0:
            self.increment_size = 32
            logger.warning(f"[DescriptorHeap] Using default increment size: {self.increment_size}")

        self._current_index = 0

        logger.info(f"[DescriptorHeap] Created: type={self.heap_type_str}, "
                    f"size={self.num_descriptors}, shader_visible={self.shader_visible}, "
                    f"cpu_start=0x{self.cpu_start:X}, gpu_start=0x{self.gpu_start:X}")

    def get_cpu_handle(self, index: int) -> int:
        """Возвращает CPU handle для дескриптора."""
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range (max {self.num_descriptors})")

        handle = self.cpu_start + (index * self.increment_size)
        logger.debug(f"[DescriptorHeap] get_cpu_handle({index}): 0x{handle:X}")
        return handle

    def get_gpu_handle(self, index: int) -> int:
        """Возвращает GPU handle для дескриптора."""
        if not self.shader_visible:
            logger.warning("[DescriptorHeap] GPU handle requested for non-shader-visible heap")
            # Возвращаем CPU handle как fallback
            return self.get_cpu_handle(index)

        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range (max {self.num_descriptors})")

        if self.use_cpu_fallback or self.gpu_start == 0:
            # В режиме fallback используем CPU handle
            cpu_handle = self.get_cpu_handle(index)
            logger.debug(f"[DescriptorHeap] Fallback mode: returning CPU handle as GPU handle: 0x{cpu_handle:X}")
            return cpu_handle

        result = self.gpu_start + (index * self.increment_size)

        # Проверяем, что handle в разумных пределах
        if result < 0x10000 or result > 0x7FFFFFFFFFFFF:
            logger.warning(f"[DescriptorHeap] Suspicious GPU handle: 0x{result:X}, using CPU fallback")
            return self.get_cpu_handle(index)

        logger.debug(f"[DescriptorHeap] get_gpu_handle({index}): 0x{result:X}")
        return result

    def next_free(self) -> int:
        """Возвращает следующий свободный индекс дескриптора."""
        if self._current_index >= self.num_descriptors:
            raise RuntimeError(f"Descriptor heap overflow: {self._current_index} >= {self.num_descriptors}")
        idx = self._current_index
        self._current_index += 1
        logger.debug(f"[DescriptorHeap] next_free: {idx}")
        return idx

    def reset(self) -> None:
        """Сбрасывает счётчик дескрипторов."""
        self._current_index = 0
        logger.debug("[DescriptorHeap] Reset")

    def __repr__(self) -> str:
        return f"DescriptorHeap(type={self.heap_type_str}, size={self.num_descriptors}, used={self._current_index})"