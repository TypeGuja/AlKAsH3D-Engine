# test.py
"""
Прямой тест Rust библиотеки alkash3d_dx12
"""

import ctypes
import os
import sys
from pathlib import Path

# Добавляем совместимость для старых версий Python
if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

print("=" * 60)
print("DIRECT TEST OF RUST DX12 LIBRARY")
print("=" * 60)

# Загружаем библиотеку
_ext = ".dll" if sys.platform.startswith("win") else ".so"
_lib_path = Path(__file__).parent / "alkash3d" / "graphics" / "utils" / f"alkash3d_dx12{_ext}"

if not _lib_path.exists():
    _lib_path = Path(__file__).parent / f"alkash3d_dx12{_ext}"

print(f"Looking for library at: {_lib_path}")
if not _lib_path.exists():
    print(f"ERROR: Library not found!")
    sys.exit(1)

# Загружаем библиотеку
lib = ctypes.CDLL(str(_lib_path))
print(f"Library loaded successfully from: {_lib_path}")


# Определяем функции
def get_func(name, restype, argtypes):
    try:
        func = getattr(lib, name)
        func.restype = restype
        func.argtypes = argtypes
        print(f"  ✓ Found function: {name}")
        return func
    except AttributeError:
        print(f"  ✗ Function not found: {name}")
        return None


print("\nChecking exported functions:")
# Основные функции
create_device = get_func("create_device", ctypes.c_void_p, [])

# Используем ctypes.c_void_p вместо c_uintptr для совместимости
create_command_queue = get_func("create_command_queue", ctypes.c_void_p, [ctypes.c_void_p])
create_swap_chain = get_func("create_swap_chain", ctypes.c_void_p,
                             [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint])
create_graphics_ps = get_func("create_graphics_ps", ctypes.c_void_p,
                              [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p])
compile_shader = get_func("compile_shader", ctypes.c_int,
                          [ctypes.c_wchar_p, ctypes.c_char_p, ctypes.c_char_p,
                           ctypes.POINTER(ctypes.c_void_p)])

if not create_device:
    print("ERROR: create_device function not found!")
    sys.exit(1)

print("\n" + "=" * 60)
print("TEST 1: Create Device")
print("=" * 60)

device = create_device()
print(f"Device created: {hex(device if device else 0)}")
if not device or device == 0:
    print("ERROR: Device creation failed!")
    sys.exit(1)

print("\n" + "=" * 60)
print("TEST 2: Create Command Queue")
print("=" * 60)

if create_command_queue:
    command_queue = create_command_queue(device)
    print(f"Command queue created: {hex(command_queue if command_queue else 0)}")
else:
    print("Command queue function not available")

print("\n" + "=" * 60)
print("TEST 3: Compile Shaders")
print("=" * 60)

# Создаем тестовые шейдеры
test_shader_dir = Path(__file__).parent / "test_shaders"
test_shader_dir.mkdir(exist_ok=True)

vs_path = test_shader_dir / "test_vert.hlsl"
ps_path = test_shader_dir / "test_frag.hlsl"

print(f"\nCreating test shaders in: {test_shader_dir}")

# Создаем тестовый вершинный шейдер
with open(vs_path, "w") as f:
    f.write("""
struct VS_INPUT
{
    float3 position : POSITION;
};

struct VS_OUTPUT
{
    float4 position : SV_POSITION;
};

VS_OUTPUT VSMain(VS_INPUT input)
{
    VS_OUTPUT output;
    output.position = float4(input.position, 1.0f);
    return output;
}
""")

# Создаем тестовый фрагментный шейдер
with open(ps_path, "w") as f:
    f.write("""
struct PS_INPUT
{
    float4 position : SV_POSITION;
};

float4 PSMain(PS_INPUT input) : SV_TARGET
{
    return float4(1.0f, 0.0f, 0.0f, 1.0f);
}
""")

print(f"Test shaders created")

if compile_shader:
    # Компилируем вершинный шейдер
    print("\nCompiling vertex shader...")
    vs_blob = ctypes.c_void_p()
    vs_path_w = ctypes.c_wchar_p(str(vs_path))
    entry = ctypes.c_char_p(b"VSMain")
    profile = ctypes.c_char_p(b"vs_5_0")

    hr = compile_shader(vs_path_w, entry, profile, ctypes.byref(vs_blob))
    print(f"  HRESULT: {hr}")
    print(f"  VS Blob: {hex(vs_blob.value if vs_blob.value else 0)}")

    if hr != 0 or not vs_blob.value:
        print("ERROR: Vertex shader compilation failed!")
        vs_blob = ctypes.c_void_p(0xDEADF00D)

    # Компилируем фрагментный шейдер
    print("\nCompiling fragment shader...")
    ps_blob = ctypes.c_void_p()
    ps_path_w = ctypes.c_wchar_p(str(ps_path))
    entry = ctypes.c_char_p(b"PSMain")
    profile = ctypes.c_char_p(b"ps_5_0")

    hr = compile_shader(ps_path_w, entry, profile, ctypes.byref(ps_blob))
    print(f"  HRESULT: {hr}")
    print(f"  PS Blob: {hex(ps_blob.value if ps_blob.value else 0)}")

    if hr != 0 or not ps_blob.value:
        print("ERROR: Fragment shader compilation failed!")
        ps_blob = ctypes.c_void_p(0xDEADF00D)

    print("\n" + "=" * 60)
    print("TEST 4: Create Graphics Pipeline")
    print("=" * 60)

    # Проверяем, что функция create_graphics_ps доступна
    if create_graphics_ps:
        print("Calling create_graphics_ps...")
        try:
            pso = create_graphics_ps(device, vs_blob, ps_blob)
            pso_val = pso.value if hasattr(pso, 'value') else int(pso) if pso else 0
            print(f"PSO created: {hex(pso_val)}")

            if pso_val and pso_val != 0 and pso_val != 0xFEEDC0DE:
                print("\n✓✓✓ SUCCESS! PSO created successfully! ✓✓✓")
                print("The Rust library is working correctly!")
            else:
                print("\n✗✗✗ FAILED! PSO creation failed! ✗✗✗")
                print("The Rust library has issues with PSO creation.")
        except Exception as e:
            print(f"Exception during PSO creation: {e}")
    else:
        print("create_graphics_ps function not available!")
else:
    print("compile_shader function not available!")

print("\n" + "=" * 60)
print("TEST COMPLETE")
print("=" * 60)