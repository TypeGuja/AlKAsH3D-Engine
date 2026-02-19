"""
Descriptor heap wrapper class.
"""

import ctypes
import traceback
from typing import Optional
from . import d3d12_wrapper as dx

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print("[DESCRIPTOR_HEAP]", *args, **kwargs)


class DescriptorHeap:
    """Wrapper for D3D12 descriptor heap."""

    _TYPE_MAP = {
        "rtv": 0,
        "dsv": 1,
        "cbv_srv_uav": 2,
    }

    def __init__(
        self,
        device: ctypes.c_void_p,
        num_descriptors: int,
        heap_type: str = "cbv_srv_uav",
    ):
        debug_print(f"DescriptorHeap.__init__(device={hex(device.value if device else 0)}, "
                    f"num={num_descriptors}, type='{heap_type}')")

        if heap_type not in self._TYPE_MAP:
            raise ValueError(f"Unsupported heap type: {heap_type}")

        # Приводим device к c_void_p (если передан «int»)
        if not isinstance(device, ctypes.c_void_p):
            try:
                device = ctypes.c_void_p(int(device))
                debug_print(f"  device converted to {hex(device.value)}")
            except Exception:
                raise TypeError(f"device must be convertible to c_void_p, got {type(device)}")

        self.device = device
        self._num_descriptors = num_descriptors
        self.heap_type = heap_type
        self._next_free = 0

        heap_type_int = self._TYPE_MAP[heap_type]

        # Для RTV и DSV кучи не должны быть шейдер-видимыми
        shader_visible = (heap_type == "cbv_srv_uav")
        debug_print(f"  heap_type_int={heap_type_int}, shader_visible={shader_visible}")

        try:
            self._heap = dx.create_descriptor_heap(device, num_descriptors, heap_type_int, shader_visible)
            if not self._heap or not self._heap.value:
                raise RuntimeError("create_descriptor_heap returned invalid pointer")
            debug_print(f"  heap created: {hex(self._heap.value)}")
        except Exception as e:
            debug_print(f"  ERROR creating heap: {e}")
            traceback.print_exc()
            raise

        try:
            self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self._heap)
            debug_print(f"  cpu_start={hex(self.cpu_start)}")
        except Exception as e:
            debug_print(f"  ERROR getting CPU handle: {e}")
            self.cpu_start = 0

        if heap_type == "cbv_srv_uav":
            try:
                self.gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self._heap)
                debug_print(f"  gpu_start={hex(self.gpu_start)}")
            except Exception as e:
                debug_print(f"  ERROR getting GPU handle: {e}")
                self.gpu_start = 0
        else:
            self.gpu_start = 0

        if heap_type == "rtv":
            try:
                self._increment_size = dx.get_rtv_descriptor_size()
                debug_print(f"  rtv increment size: {self._increment_size}")
            except Exception as e:
                debug_print(f"  ERROR getting RTV size: {e}")
                self._increment_size = 32
        elif heap_type == "dsv":
            try:
                self._increment_size = dx.get_dsv_descriptor_size()
                debug_print(f"  dsv increment size: {self._increment_size}")
            except Exception as e:
                debug_print(f"  ERROR getting DSV size: {e}")
                self._increment_size = 32
        else:
            # типичный размер для CBV/SRV/UAV‑хипа
            self._increment_size = 32
            debug_print(f"  default increment size: {self._increment_size}")

    # ------------------------------------------------------------------
    @property
    def heap(self) -> ctypes.c_void_p:
        """Raw heap pointer – нужен драйверу."""
        return self._heap

    @property
    def num_descriptors(self) -> int:
        """Количество дескрипторов – используется в Engine при росте RTV‑heap."""
        return self._num_descriptors

    # ------------------------------------------------------------------
    def next_free(self) -> int:
        debug_print(f"DescriptorHeap.next_free() - current={self._next_free}, max={self._num_descriptors}")
        if self._next_free >= self._num_descriptors:
            debug_print(f"  ERROR: Descriptor heap exhausted")
            raise RuntimeError("Descriptor heap exhausted")
        idx = self._next_free
        self._next_free += 1
        debug_print(f"  -> {idx}")
        return idx

    # ------------------------------------------------------------------
    def get_cpu_handle(self, index: int) -> int:
        debug_print(f"DescriptorHeap.get_cpu_handle(index={index})")
        if index < 0 or index >= self._num_descriptors:
            debug_print(f"  ERROR: Index {index} out of range [0, {self._num_descriptors})")
            raise ValueError(f"Index {index} out of range")
        if self.cpu_start == 0:
            result = index * self._increment_size
            debug_print(f"  -> {hex(result)} (fallback: start=0)")
            return result
        result = dx.offset_descriptor_handle(self.cpu_start, index)
        debug_print(f"  -> {hex(result)}")
        return result

    # ------------------------------------------------------------------
    def get_gpu_handle(self, index: int) -> int:
        debug_print(f"DescriptorHeap.get_gpu_handle(index={index})")
        if self.gpu_start == 0:
            debug_print(f"  -> 0 (no GPU handle)")
            return 0
        if index < 0 or index >= self._num_descriptors:
            debug_print(f"  ERROR: Index {index} out of range [0, {self._num_descriptors})")
            raise ValueError(f"Index {index} out of range")
        result = dx.offset_descriptor_handle(self.gpu_start, index)
        debug_print(f"  -> {hex(result)}")
        return result

    # ------------------------------------------------------------------
    def reset(self) -> None:
        """Сброс указателя – вызывается каждый кадр."""
        old = self._next_free
        self._next_free = 0
        debug_print(f"DescriptorHeap.reset() - {old} -> 0")