"""
Descriptor heap wrapper class.
"""

import ctypes
from typing import Optional
from . import d3d12_wrapper as dx

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
        heap_type: str = "cbv_srv_uav"
    ):
        if heap_type not in self._TYPE_MAP:
            raise ValueError(f"Unsupported heap type: {heap_type}")

        if not isinstance(device, ctypes.c_void_p):
            try:
                device = ctypes.c_void_p(int(device))
            except (TypeError, ValueError):
                raise TypeError(f"device must be convertible to c_void_p, got {type(device)}")
        self.device = device
        self.num_descriptors = num_descriptors
        self.heap_type = heap_type
        self._next_free = 0

        heap_type_int = self._TYPE_MAP[heap_type]
        self._heap = dx.create_descriptor_heap(
            device,
            num_descriptors,
            heap_type_int
        )

        if self._heap is None:
            raise RuntimeError("create_descriptor_heap returned None")

        heap_val = self._heap.value if hasattr(self._heap, 'value') else int(self._heap) if self._heap else 0
        if heap_val == 0 or heap_val == 0xDEADBEEF:
            raise RuntimeError(f"create_descriptor_heap returned invalid pointer: {hex(heap_val)}")

        self.cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self._heap)
        if heap_type == "cbv_srv_uav":
            self.gpu_start = dx.GetGPUDescriptorHandleForHeapStart(self._heap)
        else:
            self.gpu_start = 0

        if heap_type == "rtv":
            self._increment_size = dx.get_rtv_descriptor_size()
        elif heap_type == "dsv":
            self._increment_size = dx.get_dsv_descriptor_size()
        else:
            self._increment_size = 32

    @property
    def heap(self) -> ctypes.c_void_p:
        return self._heap

    def next_free(self) -> int:
        if self._next_free >= self.num_descriptors:
            raise RuntimeError("Descriptor heap exhausted")
        idx = self._next_free
        self._next_free += 1
        return idx

    def get_cpu_handle(self, index: int) -> int:
        if index < 0 or index >= self.num_descriptors:
            raise ValueError(f"Index {index} out of range")
        if self.cpu_start == 0:
            return index * self._increment_size
        return dx.offset_descriptor_handle(self.cpu_start, index)

    def get_gpu_handle(self, index: int) -> int:
        if self.gpu_start == 0:
            return 0
        if index < 0 or index >= self.num_descriptors:
            raise ValueError(f"Index {index} out of range")
        return dx.offset_descriptor_handle(self.gpu_start, index)

    def reset(self) -> None:
        self._next_free = 0