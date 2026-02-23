# alkash3d/graphics/utils/descriptor_heap.py
# -*- coding: utf-8 -*-
"""
DescriptorHeap — исправленный класс для управления кучами дескрипторов
"""

from __future__ import annotations
import ctypes
from typing import Any, Optional
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
        """
        Инициализация дескрипторной кучи

        Args:
            device: указатель на устройство DX12
            num_descriptors: количество дескрипторов
            heap_type: тип кучи ("rtv", "dsv", "cbv_srv_uav")
            shader_visible: доступна ли куча из шейдеров
        """
        self.device = device
        self.num_descriptors = int(num_descriptors)
        self.shader_visible = bool(shader_visible)
        self.heap_type_str = heap_type

        # Определяем числовой тип кучи
        self.heap_type = {
            'rtv': self.HEAP_TYPE_RTV,
            'dsv': self.HEAP_TYPE_DSV,
            'cbv_srv_uav': self.HEAP_TYPE_CBV_SRV_UAV,
        }.get(heap_type.lower(), self.HEAP_TYPE_CBV_SRV_UAV)

        logger.debug(
            f"[DescriptorHeap] Creating: type={heap_type}, num={num_descriptors}, shader_visible={shader_visible}")

        # Создаем реальную кучу через wrapper
        try:
            self.heap_ptr = dx.create_descriptor_heap(
                self.device,
                self.num_descriptors,
                self.heap_type,
                self.shader_visible
            )

            if not self.heap_ptr or not self.heap_ptr.value:
                raise RuntimeError("Failed to create descriptor heap - null pointer")

            logger.debug(f"[DescriptorHeap] Created: {hex(self.heap_ptr.value)}")

        except Exception as e:
            logger.error(f"[DescriptorHeap] Creation failed: {e}")
            raise

        # Получаем начальные handles
        try:
            self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self.heap_ptr)
            if self.cpu_start == 0:
                raise RuntimeError("Failed to get CPU handle")

            if self.shader_visible:
                self.gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self.heap_ptr)
                if self.gpu_start == 0:
                    raise RuntimeError("Failed to get GPU handle")
            else:
                self.gpu_start = 0

        except Exception as e:
            logger.error(f"[DescriptorHeap] Failed to get handles: {e}")
            raise

        # Получаем размер дескриптора
        if self.heap_type == self.HEAP_TYPE_RTV:
            self.increment_size = dx.get_rtv_descriptor_size()
        elif self.heap_type == self.HEAP_TYPE_DSV:
            self.increment_size = dx.get_dsv_descriptor_size()
        else:
            self.increment_size = dx.get_rtv_descriptor_size()  # Fallback

        if self.increment_size == 0:
            self.increment_size = 32  # Значение по умолчанию

        logger.debug(f"[DescriptorHeap] Increment size: {self.increment_size}")

        self._current_index = 0

    def next_free(self) -> int:
        """Получить следующий свободный индекс"""
        if self._current_index >= self.num_descriptors:
            raise RuntimeError(f"Descriptor heap overflow: used {self._current_index}/{self.num_descriptors}")
        idx = self._current_index
        self._current_index += 1
        logger.debug(f"[DescriptorHeap] next_free -> {idx}")
        return idx

    def get_cpu_handle(self, index: int) -> int:
        """Получить CPU handle по индексу"""
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range (max {self.num_descriptors - 1})")
        return self.cpu_start + (index * self.increment_size)

    def get_gpu_handle(self, index: int) -> int:
        """Получить GPU handle по индексу"""
        if not self.shader_visible:
            raise RuntimeError("GPU handle requested for non-shader-visible heap")
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range (max {self.num_descriptors - 1})")
        return self.gpu_start + (index * self.increment_size)

    def reset(self) -> None:
        """Сбросить индекс выделения"""
        self._current_index = 0

    def is_full(self) -> bool:
        """Проверить, заполнена ли куча"""
        return self._current_index >= self.num_descriptors

    def get_available_count(self) -> int:
        """Получить количество доступных дескрипторов"""
        return self.num_descriptors - self._current_index

    def __repr__(self) -> str:
        return f"DescriptorHeap(type={self.heap_type_str}, size={self.num_descriptors}, used={self._current_index})"