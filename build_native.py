#!/usr/bin/env python3
"""
Сборка Rust/DirectX 12 библиотеки для AlKAsH3D Engine
"""

import os
import sys
import platform
import subprocess
import shutil
from pathlib import Path


def main():
    print("🔨 Building AlKAsH3D Native Library...")

    # Проверка наличия Rust
    try:
        subprocess.run(["cargo", "--version"], check=True, capture_output=True)
        print("✅ Rust installed")
    except:
        print("❌ Rust not found! Please install Rust from https://rustup.rs/")
        return False

    root_dir = Path(__file__).parent.absolute()
    crate_dir = root_dir / "alkash3d" / "graphics" / "utils" / "alkash3d_dx12"

    if not crate_dir.exists():
        print(f"❌ Crate directory not found: {crate_dir}")
        print("Make sure you're in the correct directory")
        return False

    # Сборка
    print(f"📦 Building from: {crate_dir}")
    try:
        subprocess.run(
            ["cargo", "build", "--release"],
            cwd=crate_dir,
            check=True
        )
        print("✅ Build successful")
    except subprocess.CalledProcessError as e:
        print(f"❌ Build failed: {e}")
        return False

    # Копирование библиотеки
    system = platform.system()
    lib_suffix = {
        "Windows": ".dll",
        "Linux": ".so",
        "Darwin": ".dylib"
    }.get(system, "")

    target_dir = crate_dir / "target" / "release"
    lib_files = list(target_dir.glob(f"*{lib_suffix}"))

    if not lib_files:
        print(f"❌ Library not found in {target_dir}")
        return False

    lib_file = lib_files[0]
    dest = root_dir / lib_file.name

    shutil.copy2(lib_file, dest)
    print(f"✅ Library copied to: {dest}")
    print(f"   Size: {dest.stat().st_size / 1024:.1f} KB")

    return True


if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)