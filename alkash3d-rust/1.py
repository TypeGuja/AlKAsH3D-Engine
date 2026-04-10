import ctypes
from ctypes import wintypes
import sys

print("=" * 60)
print("Testing Rust D3D12 Backend")
print("=" * 60)

# 1. Загрузка библиотеки
print("\n1. Loading Rust library...")
try:
    rust_lib = ctypes.CDLL(
        r"C:\Users\user\Documents\GitHub\AlKAsH3D-Engine\alkash3d-rust\target\release\alkash3d_rs.dll")
    print(
        "   ✅ Found: C:\\Users\\user\\Documents\\GitHub\\AlKAsH3D-Engine\\alkash3d-rust\\target\\release\\alkash3d_rs.dll")
except Exception as e:
    print(f"   ❌ Failed to load library: {e}")
    sys.exit(1)

# 2. Настройка функций
print("\n2. Setting up functions...")

# Device functions
rust_lib.create_device.argtypes = []
rust_lib.create_device.restype = ctypes.c_void_p
rust_lib.get_gpu_name.argtypes = [ctypes.c_void_p]
rust_lib.get_gpu_name.restype = ctypes.c_char_p
rust_lib.is_warp_mode.argtypes = []
rust_lib.is_warp_mode.restype = ctypes.c_bool

# Queue functions
rust_lib.create_command_queue.argtypes = [ctypes.c_void_p]
rust_lib.create_command_queue.restype = ctypes.c_void_p

# Command functions
rust_lib.create_command_allocators.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
rust_lib.create_command_allocators.restype = ctypes.c_bool
rust_lib.create_command_list.argtypes = [ctypes.c_void_p]
rust_lib.create_command_list.restype = ctypes.c_void_p
rust_lib.create_fence.argtypes = [ctypes.c_void_p]
rust_lib.create_fence.restype = ctypes.c_bool
rust_lib.begin_frame.argtypes = []
rust_lib.begin_frame.restype = ctypes.c_bool
rust_lib.end_frame.argtypes = []
rust_lib.end_frame.restype = ctypes.c_bool
rust_lib.wait_for_gpu.argtypes = []
rust_lib.wait_for_gpu.restype = ctypes.c_bool
rust_lib.get_frame_index.argtypes = []
rust_lib.get_frame_index.restype = ctypes.c_uint32

# Heap functions
rust_lib.create_descriptor_heap.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_bool]
rust_lib.create_descriptor_heap.restype = ctypes.c_void_p
rust_lib.get_rtv_descriptor_size.argtypes = []
rust_lib.get_rtv_descriptor_size.restype = ctypes.c_uint32
rust_lib.get_cbv_srv_uav_descriptor_size.argtypes = []
rust_lib.get_cbv_srv_uav_descriptor_size.restype = ctypes.c_uint32

# Buffer functions
rust_lib.create_buffer.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
rust_lib.create_buffer.restype = ctypes.c_void_p
rust_lib.update_subresource.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
rust_lib.update_subresource.restype = ctypes.c_bool

# Cleanup function
rust_lib.release_resource.argtypes = [ctypes.c_void_p]
rust_lib.release_resource.restype = None

print("   ✅ Functions configured")

# 3. Создание устройства
print("\n3. Creating D3D12 device...")
device_ptr = rust_lib.create_device()
if not device_ptr:
    print("   ❌ Failed to create device!")
    sys.exit(1)
print(f"   ✅ Device created: {hex(device_ptr)}")

# 4. Информация о GPU
print("\n4. Checking GPU...")
gpu_name = rust_lib.get_gpu_name(device_ptr).decode('ascii', errors='ignore')
is_warp = rust_lib.is_warp_mode()
print(f"   GPU Name: {gpu_name}")
print(f"   Is WARP: {is_warp}")
print(f"   WARP Mode: {is_warp}")
if not is_warp:
    print("   ✅ Using real GPU hardware!")
else:
    print("   ⚠️ Using WARP software renderer")

# 5. Создание command queue
print("\n5. Creating command queue...")
queue_ptr = rust_lib.create_command_queue(device_ptr)
if not queue_ptr:
    print("   ❌ Failed to create command queue!")
    sys.exit(1)
print(f"   ✅ Command queue created: {hex(queue_ptr)}")

# 6. Создание command allocators (нужно 4 для triple buffering)
print("\n6. Creating command allocators...")
if not rust_lib.create_command_allocators(device_ptr, 4):
    print("   ❌ Failed to create command allocators!")
    sys.exit(1)
print("   ✅ Command allocators created (4)")

# 7. Создание command list
print("\n7. Creating command list...")
cmd_list_ptr = rust_lib.create_command_list(device_ptr)
if not cmd_list_ptr:
    print("   ❌ Failed to create command list!")
    sys.exit(1)
print(f"   ✅ Command list created: {hex(cmd_list_ptr)}")

# 8. Создание fence
print("\n8. Creating fence...")
if not rust_lib.create_fence(device_ptr):
    print("   ❌ Failed to create fence!")
    sys.exit(1)
print("   ✅ Fence created")

# 9. Создание descriptor heap
print("\n9. Creating descriptor heap...")
heap_ptr = rust_lib.create_descriptor_heap(device_ptr, 10, 2, True)
if not heap_ptr:
    print("   ❌ Failed to create descriptor heap!")
else:
    print(f"   ✅ Descriptor heap created: {hex(heap_ptr)}")

# 10. Создание vertex buffer
print("\n10. Creating vertex buffer...")
buffer_ptr = rust_lib.create_buffer(device_ptr, 144, None)
if not buffer_ptr:
    print("   ❌ Failed to create buffer!")
else:
    print(f"   ✅ Buffer created: {hex(buffer_ptr)}")

    # 11. Обновление buffer данными
    print("\n11. Updating buffer data...")
    test_data = b"Hello from Rust D3D12!" * 6
    if rust_lib.update_subresource(buffer_ptr, test_data, len(test_data)):
        print("   ✅ Buffer updated successfully!")
    else:
        print("   ❌ Buffer update failed!")

# 12. Тестирование frame команд
print("\n12. Testing frame commands...")
if rust_lib.begin_frame():
    print("   begin_frame: ✅")
    if rust_lib.end_frame():
        print("   end_frame: ✅")
        if rust_lib.wait_for_gpu():
            print("   wait_for_gpu: ✅")
        else:
            print("   wait_for_gpu: ❌")
    else:
        print("   end_frame: ❌")
else:
    print("   begin_frame: ❌")

frame_idx = rust_lib.get_frame_index()
print(f"   Frame index: {frame_idx}")

# 13. Информация о дескрипторах
print("\n13. Descriptor sizes:")
rtv_size = rust_lib.get_rtv_descriptor_size()
cbv_size = rust_lib.get_cbv_srv_uav_descriptor_size()
print(f"    RTV size: {rtv_size} bytes")
print(f"    CBV/SRV/UAV size: {cbv_size} bytes")

# 14. Очистка
print("\n14. Cleaning up...")
if heap_ptr:
    rust_lib.release_resource(ctypes.c_void_p(heap_ptr))
    print("   Heap released")
if buffer_ptr:
    rust_lib.release_resource(ctypes.c_void_p(buffer_ptr))
    print("   Buffer released")
if cmd_list_ptr:
    rust_lib.release_resource(ctypes.c_void_p(cmd_list_ptr))
    print("   Command list released")
if queue_ptr:
    rust_lib.release_resource(ctypes.c_void_p(queue_ptr))
    print("   Queue released")
if device_ptr:
    rust_lib.release_resource(ctypes.c_void_p(device_ptr))
    print("   Device released")

print("\n" + "=" * 60)
print("Test completed!")
print("=" * 60)

if not is_warp:
    print("\n✅ SUCCESS: Running on real GPU hardware!")
    print(f"   GPU: {gpu_name}")
else:
    print("\n⚠️ Running on WARP software renderer")