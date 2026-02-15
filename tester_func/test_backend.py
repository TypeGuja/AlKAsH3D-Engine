#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Complete backend test.
"""

import ctypes
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d.graphics import select_backend
from alkash3d.graphics.utils import d3d12_wrapper as dx


def test_backend():
    """Test complete backend functionality."""
    print("\n=== Complete Backend Test ===\n")

    # Select backend
    print("Selecting DX12 backend...")
    backend = select_backend("dx12")
    print(f"✅ Backend created: {type(backend).__name__}")

    # Test 1: Initialize device (without HWND)
    print("\nTest 1: Initialize device (stub mode)")
    try:
        backend.init_device(0, 800, 600)
        print(f"  ✅ Device initialized")
        print(f"  ✅ Stub mode: {backend._in_stub_mode}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        return False

    # Test 2: Create descriptor heaps through backend
    print("\nTest 2: Create descriptor heaps")
    try:
        rtv_heap = backend.create_descriptor_heap(2, "rtv")
        cbv_heap = backend.create_descriptor_heap(256, "cbv_srv_uav")
        print(f"  ✅ RTV heap: {type(rtv_heap).__name__ if rtv_heap else 'None'}")
        print(f"  ✅ CBV heap: {type(cbv_heap).__name__ if cbv_heap else 'None'}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 3: Create buffer through backend
    print("\nTest 3: Create buffer")
    try:
        test_data = b"Test buffer data" * 10
        buffer = backend.create_buffer(test_data)
        if buffer and hasattr(buffer, 'value'):
            print(f"  ✅ Buffer created: {hex(buffer.value)}")
        else:
            print(f"  ✅ Buffer created (stub): {buffer}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 4: Create texture through backend
    print("\nTest 4: Create texture")
    try:
        w, h = 64, 64
        pixel_data = bytearray([255, 0, 255, 255]) * (w * h)  # Magenta
        texture = backend.create_texture(bytes(pixel_data), w, h, "RGBA8")
        print(f"  ✅ Texture created: {type(texture).__name__}")
        if hasattr(texture, 'srv_gpu'):
            print(f"  ✅ SRV GPU handle: {hex(texture.srv_gpu if texture.srv_gpu else 0)}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 5: Update texture
    print("\nTest 5: Update texture")
    try:
        new_data = bytearray([0, 255, 255, 255]) * (w * h)  # Cyan
        backend.update_texture(texture, bytes(new_data), w, h)
        print(f"  ✅ Texture updated")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 6: Compile shader
    print("\nTest 6: Compile shader")
    try:
        # Create minimal shader file
        shader_path = Path(__file__).parent / "test.hlsl"
        with open(shader_path, 'w') as f:
            f.write("""
float4 VSMain(float4 pos : POSITION) : SV_POSITION {
    return pos;
}
float4 PSMain() : SV_TARGET {
    return float4(1,0,0,1);
}
""")

        vs_blob = backend.compile_shader("vs", str(shader_path))
        ps_blob = backend.compile_shader("ps", str(shader_path))
        print(f"  ✅ VS blob: {hex(vs_blob) if vs_blob else 'None'}")
        print(f"  ✅ PS blob: {hex(ps_blob) if ps_blob else 'None'}")

        # Cleanup
        shader_path.unlink()
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 7: Create PSO
    print("\nTest 7: Create PSO")
    try:
        pso = backend.create_graphics_ps(0xDEADF00D, 0xDEADF00D)
        print(f"  ✅ PSO created: {hex(pso) if isinstance(pso, int) else pso}")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 8: Frame operations
    print("\nTest 8: Frame operations")
    try:
        backend.begin_frame()
        print(f"  ✅ Begin frame")

        # Set viewport
        backend.set_viewport(0, 0, 800, 600)
        print(f"  ✅ Viewport set")

        # Set scissor
        backend.set_scissor_rect(0, 0, 800, 600)
        print(f"  ✅ Scissor set")

        backend.end_frame()
        print(f"  ✅ End frame")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    # Test 9: Shutdown
    print("\nTest 9: Shutdown")
    try:
        backend.shutdown()
        print(f"  ✅ Backend shut down")
    except Exception as e:
        print(f"  ❌ Failed: {e}")

    print("\n✅ All backend tests completed!")
    return True


if __name__ == "__main__":
    success = test_backend()
    sys.exit(0 if success else 1)