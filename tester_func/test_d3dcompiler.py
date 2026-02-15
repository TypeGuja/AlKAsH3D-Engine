#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Test d3dcompiler_47.dll availability.
"""

import ctypes
import sys


def test_d3dcompiler():
    """Test if d3dcompiler_47.dll is available."""
    print("\n=== Testing d3dcompiler_47.dll ===\n")

    # Try different loading methods
    dll = None
    methods = [
        ("by name", "d3dcompiler_47.dll"),
        ("from System32", "C:\\Windows\\System32\\d3dcompiler_47.dll"),
        ("from SysWOW64", "C:\\Windows\\SysWOW64\\d3dcompiler_47.dll"),
    ]

    for method_name, path in methods:
        try:
            dll = ctypes.WinDLL(path)
            print(f"✅ {method_name}: SUCCESS")
            print(f"   Loaded from: {path}")
            break
        except Exception as e:
            print(f"❌ {method_name}: FAILED - {e}")

    if not dll:
        print("\n❌ d3dcompiler_47.dll not found!")
        print("Please install Windows SDK or DirectX")
        return False

    # Check for D3DCompileFromFile function
    try:
        func = dll.D3DCompileFromFile
        print(f"\n✅ D3DCompileFromFile found at: {hex(ctypes.addressof(func))}")
        return True
    except AttributeError:
        print("\n❌ D3DCompileFromFile not found in DLL")
        return False


if __name__ == "__main__":
    success = test_d3dcompiler()
    sys.exit(0 if success else 1)