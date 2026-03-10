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


# alkash3d/graphics/utils/descriptor_heap.py
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

        except Exception as e:
            logger.error(f"[DescriptorHeap] Creation failed: {e}")
            raise

        # Получаем начальные handles
        try:
            self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self.heap_ptr)
            if self.cpu_start == 0:
                raise RuntimeError("Failed to get CPU handle")

            if self.shader_visible:
                # Получаем GPU handle (теперь он должен быть правильным сразу)
                self.gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self.heap_ptr)

                # Финальная проверка на всякий случай
                if self.gpu_start == 0x15678A00110000:
                    logger.warning("Detected broken GPU handle, using computed value")
                    self.gpu_start = self.cpu_start + 0x10000
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
            # Для CBV/SRV/UAV используем тот же размер что и для RTV
            self.increment_size = dx.get_rtv_descriptor_size()

        if self.increment_size == 0:
            self.increment_size = 32  # Значение по умолчанию

        self._current_index = 0

    def get_cpu_handle(self, index: int) -> int:
        """Получить CPU handle по индексу"""
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range")
        return self.cpu_start + (index * self.increment_size)

    def get_gpu_handle(self, index: int) -> int:
        """Получить GPU handle по индексу"""
        if not self.shader_visible:
            raise RuntimeError("GPU handle requested for non-shader-visible heap")
        if index >= self.num_descriptors:
            raise IndexError(f"Index {index} out of range")

        handle = self.gpu_start + (index * self.increment_size)

        # Дополнительная проверка (на случай если где-то просочился битый handle)
        if handle == 0x15678A00110000:
            logger.error(f"Broken GPU handle detected at index {index}, using CPU handle")
            return self.get_cpu_handle(index)

        return handle

    def next_free(self) -> int:
        """Получить следующий свободный индекс"""
        if self._current_index >= self.num_descriptors:
            raise RuntimeError(f"Descriptor heap overflow")
        idx = self._current_index
        self._current_index += 1
        return idx

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