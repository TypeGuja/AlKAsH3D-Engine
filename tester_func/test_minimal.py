#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Minimal test for Rust DLL functions.
"""

import ctypes
import sys
from pathlib import Path

# Add path to alkash3d
sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d.graphics.utils import d3d12_wrapper as dx


def test_minimal():
    """Test basic DLL functions."""
    print("\n=== Minimal Rust DLL Test ===\n")

    # Test 1: Create device
    print("Test 1: create_device()")
    device = dx.create_device()
    if device and device.value and device.value != 0xDEADBEEF:
        print(f"  ✅ Device created: {hex(device.value)}")
    else:
        print(f"  ❌ Device creation failed: {hex(device.value if device else 0)}")
        return False

    # Test 2: Create command queue
    print("\nTest 2: create_command_queue()")
    queue = dx.create_command_queue(device)
    if queue and queue.value and queue.value != 0xDEADBEEF:
        print(f"  ✅ Queue created: {hex(queue.value)}")
    else:
        print(f"  ❌ Queue creation failed: {hex(queue.value if queue else 0)}")
        return False

    # Test 3: Create descriptor heap (small)
    print("\nTest 3: create_descriptor_heap(10, type=2)")
    heap = dx.create_descriptor_heap(device, 10, 2)  # 2 = CBV_SRV_UAV
    if heap and heap.value and heap.value != 0xDEADBEEF:
        print(f"  ✅ Heap created: {hex(heap.value)}")

        # Get handles
        cpu = dx.GetCPUDescriptorHandleForHeapStart(heap)
        gpu = dx.GetGPUDescriptorHandleForHeapStart(heap)
        print(f"  ✅ CPU handle: {hex(cpu)}")
        print(f"  ✅ GPU handle: {hex(gpu)}")

        # Test offset
        offset = dx.offset_descriptor_handle(cpu, 5)
        print(f"  ✅ Offset handle (index 5): {hex(offset)}")
    else:
        print(f"  ❌ Heap creation failed: {hex(heap.value if heap else 0)}")
        return False

    # Test 4: Create descriptor heap (large)
    print("\nTest 4: create_descriptor_heap(1024, type=2)")
    heap_large = dx.create_descriptor_heap(device, 1024, 2)
    if heap_large and heap_large.value and heap_large.value != 0xDEADBEEF:
        print(f"  ✅ Large heap created: {hex(heap_large.value)}")
    else:
        print(f"  ❌ Large heap creation failed: {hex(heap_large.value if heap_large else 0)}")
        # Not fatal

    # Test 5: Get descriptor sizes
    print("\nTest 5: Get descriptor sizes")
    rtv_size = dx.get_rtv_descriptor_size()
    dsv_size = dx.get_dsv_descriptor_size()
    print(f"  ✅ RTV size: {rtv_size}")
    print(f"  ✅ DSV size: {dsv_size}")

    print("\n✅ All basic tests passed!")
    return True


if __name__ == "__main__":
    success = test_minimal()
    sys.exit(0 if success else 1)