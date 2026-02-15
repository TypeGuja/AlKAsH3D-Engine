# test_all_functions.py
"""
Тест всех экспортируемых функций Rust библиотеки
"""

import ctypes
import sys
from pathlib import Path

# Добавляем совместимость для старых версий Python
if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

print("=" * 60)
print("TESTING ALL RUST LIBRARY FUNCTIONS")
print("=" * 60)

# Загружаем библиотеку
_ext = ".dll" if sys.platform.startswith("win") else ".so"
_lib_path = Path(__file__).parent / "alkash3d" / "graphics" / "utils" / f"alkash3d_dx12{_ext}"

if not _lib_path.exists():
    _lib_path = Path(__file__).parent / f"alkash3d_dx12{_ext}"

print(f"Loading library from: {_lib_path}")
lib = ctypes.CDLL(str(_lib_path))

# Список всех функций для проверки
functions_to_test = [
    ("create_device", ctypes.c_void_p, []),
    ("create_command_queue", ctypes.c_void_p, [ctypes.c_void_p]),
    ("create_swap_chain", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]),
    ("present_swap_chain", None, [ctypes.c_void_p, ctypes.c_uint]),
    ("resize_swap_chain", None, [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]),
    ("compile_shader", ctypes.c_int,
     [ctypes.c_wchar_p, ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)]),
    ("create_graphics_ps", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]),
    ("set_graphics_pipeline", None, [ctypes.c_void_p]),
    ("create_descriptor_heap", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]),
    ("GetCPUDescriptorHandleForHeapStart", ctypes.c_uintptr, [ctypes.c_void_p]),
    ("GetGPUDescriptorHandleForHeapStart", ctypes.c_uintptr, [ctypes.c_void_p]),
    ("offset_descriptor_handle", ctypes.c_uintptr, [ctypes.c_uintptr, ctypes.c_uint]),
    ("create_buffer", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p]),
    ("update_subresource", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]),
    ("create_texture_from_memory", ctypes.c_void_p,
     [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_char_p]),
    ("update_texture", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]),
    ("create_shader_resource_view", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uintptr]),
    ("create_render_target_view", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uintptr]),
    ("set_root_descriptor_table", None, [ctypes.c_uint, ctypes.c_uintptr]),
    ("set_descriptor_heaps", None, [ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]),
    ("set_render_target", None, [ctypes.c_uintptr]),
    ("clear_render_target", None, [ctypes.c_uintptr, ctypes.POINTER(ctypes.c_float)]),
    ("set_viewport", None, [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_float, ctypes.c_float]),
    ("set_scissor_rect", None, [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int]),
    ("set_vertex_buffers", None, [ctypes.c_void_p, ctypes.c_void_p]),
    ("draw_instanced", None, [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]),
    ("draw_indexed_instanced", None, [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_uint]),
    ("wait_for_gpu", None, []),
    ("release_resource", None, [ctypes.c_void_p]),
    ("get_frame_index", ctypes.c_uint, []),
    ("get_rtv_descriptor_size", ctypes.c_uint, []),
    ("get_dsv_descriptor_size", ctypes.c_uint, []),
]

print("\nChecking functions:")
available = []
missing = []

for name, restype, argtypes in functions_to_test:
    try:
        func = getattr(lib, name)
        available.append(name)
        print(f"  ✓ {name}")
    except AttributeError:
        missing.append(name)
        print(f"  ✗ {name}")

print(f"\nAvailable functions: {len(available)}/{len(functions_to_test)}")
if missing:
    print(f"Missing functions: {', '.join(missing)}")

print("\n" + "=" * 60)
print("Creating device to test basic functionality...")
print("=" * 60)

if "create_device" in available:
    create_device = getattr(lib, "create_device")
    create_device.restype = ctypes.c_void_p
    create_device.argtypes = []

    device = create_device()
    device_val = device.value if hasattr(device, 'value') else int(device) if device else 0
    print(f"Device created: {hex(device_val)}")

    if device and device_val != 0:
        print("\n✓✓✓ BASIC TEST PASSED! Device created successfully! ✓✓✓")

        # Проверяем descriptor sizes
        if "get_rtv_descriptor_size" in available:
            get_rtv = getattr(lib, "get_rtv_descriptor_size")
            get_rtv.restype = ctypes.c_uint
            rtv_size = get_rtv()
            print(f"  RTV descriptor size: {rtv_size}")

        if "get_dsv_descriptor_size" in available:
            get_dsv = getattr(lib, "get_dsv_descriptor_size")
            get_dsv.restype = ctypes.c_uint
            dsv_size = get_dsv()
            print(f"  DSV descriptor size: {dsv_size}")
    else:
        print("\n✗✗✗ BASIC TEST FAILED! Device creation failed! ✗✗✗")
else:
    print("create_device function not available!")

print("\n" + "=" * 60)
print("TEST COMPLETE")
print("=" * 60)