# alkash3d/graphics/utils/descriptor_heap.py
# -*- coding: utf-8 -*-
"""
DescriptorHeap — удобный класс для управления кучами дескрипторов.
Делает безопасные вызовы в d3d12_wrapper с fallback'ами для отладки.
"""

from __future__ import annotations
import ctypes
from typing import Any
from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.utils import logger


class DescriptorHeap:
    HEAP_TYPE_RTV = 0
    HEAP_TYPE_DSV = 1
    HEAP_TYPE_CBV_SRV_UAV = 2

    DEFAULT_INCREMENT = 32

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
            'cbv': self.HEAP_TYPE_CBV_SRV_UAV,
            'srv': self.HEAP_TYPE_CBV_SRV_UAV,
            'uav': self.HEAP_TYPE_CBV_SRV_UAV,
        }.get(heap_type.lower(), self.HEAP_TYPE_CBV_SRV_UAV)

        logger.debug(f"[DESCRIPTOR_HEAP] DescriptorHeap.__init__(device={getattr(device, 'value', device)}, num={self.num_descriptors}, type='{self.heap_type_str}', shader_visible={self.shader_visible})")

        # Попытка создать реальную кучу через wrapper
        try:
            self.heap_ptr = dx.create_descriptor_heap(self.device, self.num_descriptors, self.heap_type, self.shader_visible)
            if not isinstance(self.heap_ptr, ctypes.c_void_p):
                self.heap_ptr = ctypes.c_void_p(int(self.heap_ptr))
            logger.debug(f"[DESCRIPTOR_HEAP] heap created: {hex(self.heap_ptr.value if self.heap_ptr and self.heap_ptr.value else 0)}")
        except Exception as e:
            logger.warning(f"[DESCRIPTOR_HEAP] create_descriptor_heap failed: {e} — using mock heap")
            self.heap_ptr = ctypes.c_void_p(0x1000 + (id(self) & 0xFFF))

        # Получаем начальные handles
        try:
            self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self.heap_ptr)
            if self.cpu_start == 0:
                # fallback: синтетический CPU handle
                self.cpu_start = 0x100000 + (id(self) & 0xFFFF)

            if self.shader_visible:
                self.gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self.heap_ptr)
                if self.gpu_start == 0:
                    self.gpu_start = 0x15678a00110000 + (id(self) & 0xFFFF)
            else:
                self.gpu_start = 0
        except Exception as e:
            logger.warning(f"[DESCRIPTOR_HEAP] handle retrieval failed: {e} — using mock handles")
            self.cpu_start = 0x100000 + (id(self) & 0xFFFF)
            self.gpu_start = 0x15678a00110000 + (id(self) & 0xFFFF)

        # Получаем размер дескриптора
        if self.heap_type == self.HEAP_TYPE_RTV:
            self.increment_size = dx.get_rtv_descriptor_size()
        elif self.heap_type == self.HEAP_TYPE_DSV:
            self.increment_size = dx.get_dsv_descriptor_size()
        else:
            self.increment_size = self.DEFAULT_INCREMENT

        self._current_index = 0

    def next_free(self) -> int:
        if self._current_index >= self.num_descriptors:
            raise RuntimeError("Descriptor heap overflow")
        idx = self._current_index
        self._current_index += 1
        logger.debug(f"[DESCRIPTOR_HEAP] next_free -> {idx}")
        return idx

    def get_cpu_handle(self, index: int) -> int:
        if index >= self.num_descriptors:
            raise IndexError("Index out of range")
        return self.cpu_start + (index * self.increment_size)

    def get_gpu_handle(self, index: int) -> int:
        if not self.shader_visible:
            raise RuntimeError("GPU handle requested for non-shader-visible heap")
        if index >= self.num_descriptors:
            raise IndexError("Index out of range")
        return self.gpu_start + (index * self.increment_size)

    def offset_cpu_handle(self, base_handle: int, offset_in_descriptors: int) -> int:
        return base_handle + (offset_in_descriptors * self.increment_size)

    def offset_gpu_handle(self, base_handle: int, offset_in_descriptors: int) -> int:
        return base_handle + (offset_in_descriptors * self.increment_size)

    def reset(self) -> None:
        self._current_index = 0

    def is_full(self) -> bool:
        return self._current_index >= self.num_descriptors

    def get_available_count(self) -> int:
        return self.num_descriptors - self._current_index

    def __repr__(self) -> str:
        ptr_hex = hex(self.heap_ptr.value if isinstance(self.heap_ptr, ctypes.c_void_p) and self.heap_ptr.value else 0)
        return f"DescriptorHeap(type={self.heap_type_str}, size={self.num_descriptors}, used={self._current_index}, ptr={ptr_hex})"