import ctypes
from ctypes import wintypes
import os
import sys

# Загружаем DLL
dll_path = os.path.join('../alkash3d', 'alkash3d_dx12', 'target', 'release', 'alkash3d_dx12.dll')
print(f"Загрузка DLL: {dll_path}")

try:
    dll = ctypes.CDLL(dll_path)
    print("✓ DLL успешно загружена")
except Exception as e:
    print(f"✗ Ошибка загрузки DLL: {e}")
    sys.exit(1)

# Определяем константы
D3D12_DESCRIPTOR_HEAP_TYPE_RTV = 0
D3D12_DESCRIPTOR_HEAP_TYPE_DSV = 1
D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV = 2

# Определяем сигнатуры функций
create_device = dll.create_device
create_device.argtypes = []
create_device.restype = ctypes.c_void_p

create_command_queue = dll.create_command_queue
create_command_queue.argtypes = [ctypes.c_void_p]
create_command_queue.restype = ctypes.c_void_p

create_descriptor_heap = dll.create_descriptor_heap
create_descriptor_heap.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32]
create_descriptor_heap.restype = ctypes.c_void_p

GetCPUDescriptorHandleForHeapStart = dll.GetCPUDescriptorHandleForHeapStart
GetCPUDescriptorHandleForHeapStart.argtypes = [ctypes.c_void_p]
GetCPUDescriptorHandleForHeapStart.restype = ctypes.c_size_t

release_resource = dll.release_resource
release_resource.argtypes = [ctypes.c_void_p]
release_resource.restype = None

get_root_signature = dll.get_root_signature
get_root_signature.argtypes = []
get_root_signature.restype = ctypes.c_void_p

get_frame_index = dll.get_frame_index
get_frame_index.argtypes = []
get_frame_index.restype = ctypes.c_uint32

get_rtv_descriptor_size = dll.get_rtv_descriptor_size
get_rtv_descriptor_size.argtypes = []
get_rtv_descriptor_size.restype = ctypes.c_uint32

get_dsv_descriptor_size = dll.get_dsv_descriptor_size
get_dsv_descriptor_size.argtypes = []
get_dsv_descriptor_size.restype = ctypes.c_uint32

create_buffer = dll.create_buffer
create_buffer.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p]
create_buffer.restype = ctypes.c_void_p

update_buffer = dll.update_buffer
update_buffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
update_buffer.restype = None

compile_shader = dll.compile_shader
compile_shader.argtypes = [
    ctypes.c_wchar_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.POINTER(ctypes.c_void_p)
]
compile_shader.restype = ctypes.c_int

create_graphics_ps = dll.create_graphics_ps
create_graphics_ps.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
create_graphics_ps.restype = ctypes.c_void_p

print("\n=== ПРОВЕРКА ЭКСПОРТИРУЕМЫХ ФУНКЦИЙ ===\n")

# Список всех функций для проверки
functions = [
    "create_device",
    "create_command_queue",
    "create_swap_chain",
    "present",
    "present_swap_chain",
    "resize_swap_chain",
    "create_descriptor_heap",
    "GetCPUDescriptorHandleForHeapStart",
    "GetGPUDescriptorHandleForHeapStart",
    "offset_descriptor_handle",
    "create_buffer",
    "update_buffer",
    "update_subresource",
    "create_texture_from_memory",
    "update_texture",
    "create_shader_resource_view",
    "create_render_target_view",
    "compile_shader",
    "create_graphics_ps",
    "set_graphics_pipeline",
    "set_root_descriptor_table",
    "set_descriptor_heaps",
    "set_render_target",
    "set_render_targets",
    "clear_render_target",
    "set_viewport",
    "set_scissor_rect",
    "set_vertex_buffers",
    "draw_instanced",
    "draw_indexed_instanced",
    "wait_for_gpu",
    "release_resource",
    "get_root_signature",
    "get_frame_index",
    "get_rtv_descriptor_size",
    "get_dsv_descriptor_size",
]

success_count = 0
fail_count = 0

for func_name in functions:
    try:
        func = getattr(dll, func_name)
        print(f"✓ {func_name:35} - найдена")
        success_count += 1
    except AttributeError:
        print(f"✗ {func_name:35} - НЕ НАЙДЕНА!")
        fail_count += 1

print(f"\n=== ИТОГ ===")
print(f"Найдено функций: {success_count}")
print(f"Отсутствует: {fail_count}")
print(f"Всего: {len(functions)}")

print("\n=== ТЕСТ БАЗОВОГО ФУНКЦИОНАЛА ===\n")

try:
    # 1. Создаем устройство
    device = create_device()
    if device:
        print(f"✓ create_device: {hex(device)}")
    else:
        print("✗ create_device: ОШИБКА")
        sys.exit(1)

    # 2. Создаем очередь команд
    command_queue = create_command_queue(device)
    if command_queue:
        print(f"✓ create_command_queue: {hex(command_queue)}")
    else:
        print("✗ create_command_queue: ОШИБКА")

    # 3. Получаем root signature
    root_sig = get_root_signature()
    if root_sig:
        print(f"✓ get_root_signature: {hex(root_sig)}")
    else:
        print("✗ get_root_signature: ОШИБКА")

    # 4. Проверяем дескрипторные размеры
    rtv_size = get_rtv_descriptor_size()
    dsv_size = get_dsv_descriptor_size()
    print(f"✓ get_rtv_descriptor_size: {rtv_size}")
    print(f"✓ get_dsv_descriptor_size: {dsv_size}")

    # 5. Проверяем индекс кадра
    frame_index = get_frame_index()
    print(f"✓ get_frame_index: {frame_index}")

    # 6. Создаем буфер
    buffer = create_buffer(device, 1024, b"vertex")
    if buffer:
        print(f"✓ create_buffer: {hex(buffer)}")

        # Создаем тестовые данные
        import array

        test_data = array.array('B', [i % 256 for i in range(1024)])
        data_ptr = (ctypes.c_ubyte * 1024).from_buffer(test_data)

        # Обновляем буфер
        update_buffer(buffer, ctypes.addressof(data_ptr), 1024)
        print("✓ update_buffer")

        # Освобождаем буфер
        release_resource(buffer)
        print("✓ release_resource (buffer)")
    else:
        print("✗ create_buffer: ОШИБКА")

    # 7. Тест компиляции шейдера
    import tempfile

    hlsl_code = """
    struct VSInput
    {
        float4 position : POSITION;
        float4 color : COLOR;
    };

    struct PSInput
    {
        float4 position : SV_POSITION;
        float4 color : COLOR;
    };

    PSInput VSMain(VSInput input)
    {
        PSInput result;
        result.position = input.position;
        result.color = input.color;
        return result;
    }

    float4 PSMain(PSInput input) : SV_TARGET
    {
        return input.color;
    }
    """

    temp_dir = tempfile.mkdtemp()
    shader_path = os.path.join(temp_dir, "test.hlsl")
    with open(shader_path, 'w', encoding='utf-8') as f:
        f.write(hlsl_code)

    print(f"\n✓ Создан временный шейдер: {shader_path}")

    # Компилируем вершинный шейдер
    vs_blob = ctypes.c_void_p()
    result = compile_shader(
        shader_path,
        b"VSMain",
        b"vs_5_0",
        ctypes.byref(vs_blob)
    )

    if result == 0 and vs_blob.value:
        print("✓ compile_shader (VS) - успешно")
    else:
        print(f"✗ compile_shader (VS): {result}")

    # Компилируем пиксельный шейдер
    ps_blob = ctypes.c_void_p()
    result = compile_shader(
        shader_path,
        b"PSMain",
        b"ps_5_0",
        ctypes.byref(ps_blob)
    )

    if result == 0 and ps_blob.value:
        print("✓ compile_shader (PS) - успешно")
    else:
        print(f"✗ compile_shader (PS): {result}")

    # Создаем pipeline state object
    if vs_blob.value and ps_blob.value:
        pso = create_graphics_ps(device, vs_blob.value, ps_blob.value)
        if pso:
            print(f"✓ create_graphics_ps: {hex(pso)}")
            release_resource(pso)
            print("✓ release_resource (PSO)")
        else:
            print("✗ create_graphics_ps: ОШИБКА")

    # Очищаем временные файлы
    os.remove(shader_path)
    os.rmdir(temp_dir)

    # Освобождаем шейдерные блобы
    if vs_blob.value:
        release_resource(vs_blob.value)
        print("✓ release_resource (VS blob)")
    if ps_blob.value:
        release_resource(ps_blob.value)
        print("✓ release_resource (PS blob)")

    # Освобождаем ресурсы
    if command_queue:
        release_resource(command_queue)
        print("✓ release_resource (command queue)")

    if device:
        release_resource(device)
        print("✓ release_resource (device)")

    print("\n=== ТЕСТ УСПЕШНО ЗАВЕРШЕН ===")

except Exception as e:
    print(f"\n✗ Ошибка при выполнении теста: {e}")
    import traceback

    traceback.print_exc()