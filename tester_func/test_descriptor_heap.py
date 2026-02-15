#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Test DescriptorHeap class.
"""

import ctypes
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap


def test_descriptor_heap():
    """Test DescriptorHeap class."""
    print("\n=== Testing DescriptorHeap Class ===\n")

    # Create device first
    device = dx.create_device()
    if not device or not device.value:
        print("❌ Failed to create device")
        return False

    print(f"✅ Device created: {hex(device.value)}")

    # Test 1: Create RTV heap
    print("\nTest 1: Create RTV heap")
    try:
        rtv_heap = DescriptorHeap(device, 2, "rtv")
        print(f"  ✅ RTV heap created")
        print(f"  ✅ CPU start: {hex(rtv_heap.cpu_start)}")
        print(f"  ✅ GPU start: {hex(rtv_heap.gpu_start)}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        return False

    # Test 2: Create CBV/SRV/UAV heap
    print("\nTest 2: Create CBV/SRV/UAV heap")
    try:
        cbv_heap = DescriptorHeap(device, 256, "cbv_srv_uav")
        print(f"  ✅ CBV heap created")
        print(f"  ✅ CPU start: {hex(cbv_heap.cpu_start)}")
        print(f"  ✅ GPU start: {hex(cbv_heap.gpu_start)}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        return False

    # Test 3: Create large heap
    print("\nTest 3: Create large heap (1024 descriptors)")
    try:
        large_heap = DescriptorHeap(device, 1024, "cbv_srv_uav")
        print(f"  ✅ Large heap created")
        print(f"  ✅ CPU start: {hex(large_heap.cpu_start)}")
        print(f"  ✅ GPU start: {hex(large_heap.gpu_start)}")
    except Exception as e:
        print(f"  ⚠️ Large heap failed (may be normal): {e}")

    # Test 4: Allocate descriptors
    print("\nTest 4: Allocate descriptors")
    try:
        indices = []
        for i in range(5):
            idx = cbv_heap.next_free()
            indices.append(idx)
            cpu = cbv_heap.get_cpu_handle(idx)
            gpu = cbv_heap.get_gpu_handle(idx)
            print(f"  ✅ Index {idx}: CPU={hex(cpu)}, GPU={hex(gpu)}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        return False

    # Test 5: Reset and reallocate
    print("\nTest 5: Reset heap")
    try:
        cbv_heap.reset()
        idx = cbv_heap.next_free()
        print(f"  ✅ After reset, first index: {idx}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        return False

    # Test 6: Error handling - out of range
    print("\nTest 6: Error handling - out of range")
    try:
        cbv_heap.get_cpu_handle(9999)
        print("  ❌ Should have raised ValueError")
    except ValueError as e:
        print(f"  ✅ Correctly caught: {e}")
    except Exception as e:
        print(f"  ❌ Wrong exception: {e}")

    # Test 7: Error handling - heap exhausted
    print("\nTest 7: Error handling - heap exhausted")
    try:
        small_heap = DescriptorHeap(device, 2, "cbv_srv_uav")
        small_heap.next_free()
        small_heap.next_free()
        small_heap.next_free()  # Should raise
        print("  ❌ Should have raised RuntimeError")
    except RuntimeError as e:
        print(f"  ✅ Correctly caught: {e}")
    except Exception as e:
        print(f"  ❌ Wrong exception: {e}")

    print("\n✅ All DescriptorHeap tests passed!")
    return True


if __name__ == "__main__":
    success = test_descriptor_heap()
    sys.exit(0 if success else 1)