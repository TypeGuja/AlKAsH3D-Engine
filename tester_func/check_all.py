#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Run all tests.
"""

import subprocess
import sys
from pathlib import Path


def run_test(script_name):
    """Run a single test script."""
    print(f"\n{'=' * 60}")
    print(f"Running: {script_name}")
    print(f"{'=' * 60}")

    script_path = Path(__file__).parent / script_name
    result = subprocess.run([sys.executable, str(script_path)],
                            capture_output=True, text=True)

    print(result.stdout)
    if result.stderr:
        print("STDERR:", result.stderr)

    return result.returncode == 0


def main():
    """Run all tests."""
    print("=" * 60)
    print("Running All DirectX 12 Tests")
    print("=" * 60)

    tests = [
        "test_d3dcompiler.py",
        "test_minimal.py",
        "test_descriptor_heap.py",
        "test_shader_compilation.py",
        "test_buffer_texture.py",
        "test_backend.py",
    ]

    passed = 0
    failed = 0

    for test in tests:
        if run_test(test):
            print(f"✅ {test} PASSED")
            passed += 1
        else:
            print(f"❌ {test} FAILED")
            failed += 1

    print(f"\n{'=' * 60}")
    print(f"Results: {passed} passed, {failed} failed")
    print(f"{'=' * 60}")

    return failed == 0


if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)