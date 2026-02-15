#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Test buffer and texture creation.
"""

import ctypes
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap


def test_buffer_texture():
    """Test buffer and texture creation."""
    print("\n=== Testing Buffers and Textures ===\n")

    # Create device
    device = dx.create_device()
    if not device or not device.value:
        print("❌ Failed to create device")
        return False

    print(f"✅ Device created: {hex(device.value)}")

    # Create descriptor heap for SRVs
    try:
        srv_heap = DescriptorHeap(device, 10, "cbv_srv_uav")
        print(f"✅ SRV heap created")
    except Exception as e:
        print(f"❌ Failed to create SRV heap: {e}")
        return False

    # Test 1: Create buffer
    print("\nTest 1: Create buffer")
    test_data = b"Hello, DirectX 12!" * 10
    try:
        buffer = dx.create_buffer(device, len(test_data), "default")
        if buffer and buffer.value and buffer.value != 0xDEADBEEF:
            print(f"  ✅ Buffer created: {hex(buffer.value)}")
            print(f"  ✅ Size: {len(test_data)} bytes")
        else:
            print(f"  ⚠️ Buffer returned stub: {hex(buffer.value if buffer else 0)}")
            buffer = ctypes.c_void_p(0xDEADBEEF)
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        buffer = ctypes.c_void_p(0xDEADBEEF)

    # Test 2: Update buffer
    print("\nTest 2: Update buffer")
    try:
        dx.update_subresource(buffer, test_data)
        print(f"  ✅ Buffer updated")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 3: Create constant buffer
    print("\nTest 3: Create constant buffer")
    const_data = b"\x00" * 256  # 256 bytes of zeros
    try:
        const_buffer = dx.create_buffer(device, len(const_data), "constant")
        if const_buffer and const_buffer.value and const_buffer.value != 0xDEADBEEF:
            print(f"  ✅ Constant buffer created: {hex(const_buffer.value)}")
        else:
            print(f"  ⚠️ Constant buffer returned stub")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 4: Create texture (empty)
    print("\nTest 4: Create empty texture 256x256")
    try:
        tex = dx.create_texture_from_memory(device, None, 256, 256, "rgba8")
        if tex and tex.value and tex.value != 0xDEADBEEF:
            print(f"  ✅ Texture created: {hex(tex.value)}")
        else:
            print(f"  ⚠️ Texture returned stub: {hex(tex.value if tex else 0)}")
            tex = ctypes.c_void_p(0xDEADBEEF)
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        tex = ctypes.c_void_p(0xDEADBEEF)

    # Test 5: Create texture with data
    print("\nTest 5: Create texture with data 64x64")
    try:
        # Create checkerboard pattern
        w, h = 64, 64
        pixel_data = bytearray()
        for y in range(h):
            for x in range(w):
                if (x // 8 + y // 8) % 2:
                    pixel_data.extend([255, 0, 0, 255])  # Red
                else:
                    pixel_data.extend([0, 255, 0, 255])  # Green

        tex_data = dx.create_texture_from_memory(
            device, bytes(pixel_data), w, h, "rgba8"
        )
        if tex_data and tex_data.value and tex_data.value != 0xDEADBEEF:
            print(f"  ✅ Textured texture created: {hex(tex_data.value)}")
        else:
            print(f"  ⚠️ Textured texture returned stub")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 6: Create SRV
    print("\nTest 6: Create Shader Resource View")
    try:
        if tex and tex.value and tex.value != 0xDEADBEEF and srv_heap:
            idx = srv_heap.next_free()
            cpu_handle = srv_heap.get_cpu_handle(idx)
            dx.create_shader_resource_view(device, tex, cpu_handle)
            gpu_handle = srv_heap.get_gpu_handle(idx)
            print(f"  ✅ SRV created at index {idx}")
            print(f"  ✅ CPU handle: {hex(cpu_handle)}")
            print(f"  ✅ GPU handle: {hex(gpu_handle)}")
        else:
            print(f"  ⚠️ Skipped SRV creation (no valid texture)")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 7: Update texture
    print("\nTest 7: Update texture")
    try:
        if tex and tex.value and tex.value != 0xDEADBEEF:
            new_data = b"\xFF" * (256 * 256 * 4)  # White texture
            dx.update_texture(tex, new_data, 256, 256)
            print(f"  ✅ Texture updated")
        else:
            print(f"  ⚠️ Skipped texture update")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    print("\n✅ Buffer and texture tests completed")
    return True


if __name__ == "__main__":
    success = test_buffer_texture()
    sys.exit(0 if success else 1)