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
        if self.shader_visible:
            raw_gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self.heap_ptr)

            # Проверяем на битый handle от WARP драйвера
            BROKEN_HANDLES = [0, 0x15678A00110000, 0x25678A00120000]
            if raw_gpu_start in BROKEN_HANDLES or raw_gpu_start < 0x1000000000000:
                # Вычисляем правильный GPU handle
                # CPU и GPU handles обычно отличаются на 0x10000
                self.gpu_start = self.cpu_start + 0x10000
                logger.warning(f"[DescriptorHeap] Driver returned broken GPU handle 0x{raw_gpu_start:X}")
                logger.info(f"[DescriptorHeap] Using computed GPU start: 0x{self.gpu_start:X}")
            else:
                self.gpu_start = raw_gpu_start
                logger.info(f"[DescriptorHeap] GPU start: 0x{self.gpu_start:X}")
        else:
            self.gpu_start = 0

        # Размер дескриптора
        if self.heap_type == self.HEAP_TYPE_RTV:
            self.increment_size = dx.get_rtv_descriptor_size()
        elif self.heap_type == self.HEAP_TYPE_DSV:
            self.increment_size = dx.get_dsv_descriptor_size()
        else:
            self.increment_size = dx.get_rtv_descriptor_size()

        if self.increment_size == 0:
            self.increment_size = 32

        self._current_index = 0

    def get_cpu_handle(self, index: int) -> int:
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range")
        return self.cpu_start + (index * self.increment_size)

    def get_gpu_handle(self, index: int) -> int:
        if not self.shader_visible:
            raise RuntimeError("GPU handle requested for non-shader-visible heap")
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range")
        return self.gpu_start + (index * self.increment_size)

    def next_free(self) -> int:
        if self._current_index >= self.num_descriptors:
            raise RuntimeError("Descriptor heap overflow")
        idx = self._current_index
        self._current_index += 1
        return idx

    def reset(self) -> None:
        self._current_index = 0

    def __repr__(self) -> str:
        return f"DescriptorHeap(type={self.heap_type_str}, size={self.num_descriptors}, used={self._current_index})"