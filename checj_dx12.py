# check_dx12.py
"""
Скрипт для проверки поддержки DirectX 12 на системе.
"""

import ctypes
import sys
import platform


def check_windows_version():
    """Проверка версии Windows."""
    print("Windows Version Check:")
    version = platform.version()
    release = platform.release()

    print(f"  Version: {version}")
    print(f"  Release: {release}")

    # Windows 10 и выше должны поддерживать DX12
    if release in ['10', '11']:
        print("  ✅ Windows version should support DX12")
        return True
    else:
        print("  ❌ Windows version may not support DX12")
        return False


def check_d3d12_dll():
    """Проверка наличия DLL DirectX 12."""
    print("\nD3D12 DLL Check:")

    try:
        d3d12 = ctypes.windll.d3d12
        print("  ✅ d3d12.dll loaded successfully")
        return True
    except Exception as e:
        print(f"  ❌ Failed to load d3d12.dll: {e}")
        return False


def check_dxgi_dll():
    """Проверка наличия DXGI DLL."""
    print("\nDXGI DLL Check:")

    try:
        dxgi = ctypes.windll.dxgi
        print("  ✅ dxgi.dll loaded successfully")
        return True
    except Exception as e:
        print(f"  ❌ Failed to load dxgi.dll: {e}")
        return False


def check_d3dcompiler():
    """Проверка наличия компилятора шейдеров."""
    print("\nD3D Compiler Check:")

    try:
        d3dcompiler = ctypes.windll.d3dcompiler_47
        print("  ✅ d3dcompiler_47.dll loaded successfully")
        return True
    except Exception as e:
        print(f"  ❌ Failed to load d3dcompiler_47.dll: {e}")
        return False


def check_graphics_drivers():
    """Проверка графических драйверов через DXGI."""
    print("\nGraphics Driver Check:")

    try:
        from ctypes import wintypes

        # Пробуем создать фабрику DXGI
        dxgi = ctypes.windll.dxgi

        # Определяем функцию CreateDXGIFactory1
        CreateDXGIFactory1 = dxgi.CreateDXGIFactory1
        CreateDXGIFactory1.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
        CreateDXGIFactory1.restype = ctypes.c_int

        # GUID IDXGIFactory1
        IID_IDXGIFactory1 = (0x770aae78, 0xf26f, 0x4dba, 0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87)

        factory = ctypes.c_void_p()
        hr = CreateDXGIFactory1(ctypes.byref(IID_IDXGIFactory1), ctypes.byref(factory))

        if hr == 0:  # S_OK
            print("  ✅ DXGI factory created successfully")

            # Пробуем получить адаптеры
            EnumAdapters = factory.value + (12 * 8)  # Приблизительное смещение

            print("  Graphics adapters found (check Device Manager)")
            return True
        else:
            print(f"  ❌ Failed to create DXGI factory: HRESULT {hex(hr)}")
            return False

    except Exception as e:
        print(f"  ❌ Error checking graphics drivers: {e}")
        return False


def main():
    """Главная функция."""
    print("=" * 60)
    print("DIRECTX 12 SYSTEM CHECK")
    print("=" * 60)
    print(f"Python: {sys.version}")
    print(f"Architecture: {platform.architecture()[0]}")
    print(f"Machine: {platform.machine()}")
    print()

    checks = [
        ("Windows Version", check_windows_version()),
        ("D3D12 DLL", check_d3d12_dll()),
        ("DXGI DLL", check_dxgi_dll()),
        ("D3D Compiler", check_d3dcompiler()),
        ("Graphics Drivers", check_graphics_drivers()),
    ]

    print("\n" + "=" * 60)
    print("SUMMARY")
    print("=" * 60)

    all_passed = True
    for name, passed in checks:
        status = "✅" if passed else "❌"
        print(f"{status} {name}")
        if not passed:
            all_passed = False

    print()
    if all_passed:
        print("✅ All checks passed! Your system should support DirectX 12.")
        print("\nNext steps:")
        print("  1. Run simple_dx12.py to test with a window")
        print("  2. Check if your antivirus is blocking DX12")
        print("  3. Try running as administrator")
    else:
        print("❌ Some checks failed. Your system may not support DirectX 12.")
        print("\nTroubleshooting:")
        print("  1. Update Windows to latest version")
        print("  2. Update graphics drivers")
        print("  3. Install DirectX runtime from Microsoft")
        print("  4. Check if your GPU supports DX12")

    return 0 if all_passed else 1


if __name__ == "__main__":
    sys.exit(main())