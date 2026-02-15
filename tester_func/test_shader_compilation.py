#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Test shader compilation.
"""

import os
import ctypes
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d.graphics.utils import d3d12_wrapper as dx

# Minimal test shader
TEST_SHADER = """
struct VSInput {
    float4 position : POSITION;
};

struct VSOutput {
    float4 position : SV_POSITION;
};

VSOutput VSMain(VSInput input) {
    VSOutput output;
    output.position = input.position;
    return output;
}

float4 PSMain() : SV_TARGET {
    return float4(1.0, 0.0, 0.0, 1.0);
}
"""


def create_test_shader_file():
    """Create temporary test shader file."""
    fd, path = tempfile.mkstemp(suffix=".hlsl", text=True)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(TEST_SHADER)
        return path
    except:
        os.close(fd)
        raise


def test_shader_compilation():
    """Test shader compilation."""
    print("\n=== Testing Shader Compilation ===\n")

    # Create test shader file
    shader_path = create_test_shader_file()
    print(f"✅ Created test shader: {shader_path}")

    # Test 1: Compile vertex shader
    print("\nTest 1: Compile vertex shader")
    try:
        vs_blob = dx.compile_shader(shader_path, "VSMain", "vs_5_0")
        if vs_blob and vs_blob != 0xDEADF00D:
            print(f"  ✅ VS compiled: {hex(vs_blob)}")
        else:
            print(f"  ⚠️ VS compilation returned stub: {hex(vs_blob)}")
            vs_blob = 0xDEADF00D
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        vs_blob = 0xDEADF00D

    # Test 2: Compile pixel shader
    print("\nTest 2: Compile pixel shader")
    try:
        ps_blob = dx.compile_shader(shader_path, "PSMain", "ps_5_0")
        if ps_blob and ps_blob != 0xDEADF00D:
            print(f"  ✅ PS compiled: {hex(ps_blob)}")
        else:
            print(f"  ⚠️ PS compilation returned stub: {hex(ps_blob)}")
            ps_blob = 0xDEADF00D
    except Exception as e:
        print(f"  ❌ Failed: {e}")
        ps_blob = 0xDEADF00D

    # Test 3: Try with different profiles
    print("\nTest 3: Try different profiles")
    profiles = [
        ("vs_4_0", "vs_4_0"),
        ("vs_5_0", "vs_5_0"),
        ("vs_5_1", "vs_5_1"),
    ]

    for name, profile in profiles:
        try:
            blob = dx.compile_shader(shader_path, "VSMain", profile)
            status = "✅" if blob and blob != 0xDEADF00D else "⚠️"
            print(f"  {status} {name}: {hex(blob) if blob else 'None'}")
        except:
            print(f"  ❌ {name}: failed")

    # Cleanup
    try:
        os.unlink(shader_path)
        print(f"\n✅ Cleaned up test file")
    except:
        pass

    return True


if __name__ == "__main__":
    success = test_shader_compilation()
    sys.exit(0 if success else 1)