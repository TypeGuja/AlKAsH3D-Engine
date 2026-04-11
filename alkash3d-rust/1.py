"""
Простой тест для проверки Rust D3D12 бэкенда
"""

import ctypes
from pathlib import Path

def main():
    print("=" * 60)
    print("Testing Rust D3D12 Backend")
    print("=" * 60)

    # 1. Загружаем библиотеку
    print("\n1. Loading Rust library...")
    lib_path = Path(__file__).parent / "target" / "release" / "alkash3d_rs.dll"

    if not lib_path.exists():
        lib_path = Path("alkash3d_rs.dll")

    if not lib_path.exists():
        print(f"   ❌ Library not found: {lib_path}")
        return

    print(f"   ✅ Found: {lib_path}")
    lib = ctypes.CDLL(str(lib_path))

    # 2. Настраиваем функции
    print("\n2. Setting up functions...")

    lib.create_device.argtypes = []
    lib.create_device.restype = ctypes.c_void_p

    lib.check_warp_driver.argtypes = [ctypes.c_void_p]
    lib.check_warp_driver.restype = ctypes.c_bool

    lib.is_warp_mode.argtypes = []
    lib.is_warp_mode.restype = ctypes.c_bool

    lib.get_gpu_name.argtypes = [ctypes.c_void_p]
    lib.get_gpu_name.restype = ctypes.c_char_p

    lib.create_command_queue.argtypes = [ctypes.c_void_p]
    lib.create_command_queue.restype = ctypes.c_void_p

    lib.create_descriptor_heap.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_bool]
    lib.create_descriptor_heap.restype = ctypes.c_void_p

    lib.GetGPUDescriptorHandleForHeapStart.argtypes = [ctypes.c_void_p]
    lib.GetGPUDescriptorHandleForHeapStart.restype = ctypes.c_uint64

    lib.GetCPUDescriptorHandleForHeapStart.argtypes = [ctypes.c_void_p]
    lib.GetCPUDescriptorHandleForHeapStart.restype = ctypes.c_uint64

    lib.create_buffer.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    lib.create_buffer.restype = ctypes.c_void_p

    lib.update_subresource.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
    lib.update_subresource.restype = ctypes.c_bool

    lib.begin_frame.argtypes = []
    lib.begin_frame.restype = ctypes.c_bool

    lib.end_frame.argtypes = []
    lib.end_frame.restype = ctypes.c_bool

    lib.wait_for_gpu.argtypes = []
    lib.wait_for_gpu.restype = ctypes.c_bool

    lib.get_frame_index.argtypes = []
    lib.get_frame_index.restype = ctypes.c_uint32

    lib.get_rtv_descriptor_size.argtypes = []
    lib.get_rtv_descriptor_size.restype = ctypes.c_uint32

    lib.get_cbv_srv_uav_descriptor_size.argtypes = []
    lib.get_cbv_srv_uav_descriptor_size.restype = ctypes.c_uint32

    lib.release_resource.argtypes = [ctypes.c_void_p]
    lib.release_resource.restype = None

    lib.force_cleanup.argtypes = []
    lib.force_cleanup.restype = None

    print("   ✅ Functions configured")

    # 3. Создаём устройство
    print("\n3. Creating D3D12 device...")
    device = lib.create_device()

    if device == 0:
        print("   ❌ Failed to create device!")
        return

    print(f"   ✅ Device created: 0x{device:X}")

    # 4. Проверяем WARP
    print("\n4. Checking GPU...")
    is_warp = lib.check_warp_driver(device)
    warp_mode = lib.is_warp_mode()

    gpu_name = lib.get_gpu_name(device)
    if gpu_name:
        print(f"   GPU Name: {gpu_name.decode('utf-8')}")

    print(f"   Is WARP: {is_warp}")
    print(f"   WARP Mode: {warp_mode}")

    if is_warp:
        print("   ⚠️  WARNING: Using software renderer (WARP)")
        print("   Performance will be poor!")
    else:
        print("   ✅ Using real GPU hardware!")

    # 5. Создаём командную очередь
    print("\n5. Creating command queue...")
    queue = lib.create_command_queue(device)

    if queue == 0:
        print("   ❌ Failed to create command queue!")
        lib.release_resource(device)
        return

    print(f"   ✅ Command queue created: 0x{queue:X}")

    # 6. Создаём дескрипторную кучу
    print("\n6. Creating descriptor heap...")
    heap = lib.create_descriptor_heap(device, 10, 2, not is_warp)

    if heap == 0:
        print("   ❌ Failed to create descriptor heap!")
    else:
        print(f"   ✅ Descriptor heap created: 0x{heap:X}")

        cpu_handle = lib.GetCPUDescriptorHandleForHeapStart(heap)
        print(f"   CPU handle: 0x{cpu_handle:X}")

        if not is_warp:
            gpu_handle = lib.GetGPUDescriptorHandleForHeapStart(heap)
            print(f"   GPU handle: 0x{gpu_handle:X}")

            if gpu_handle != 0 and gpu_handle > 0x10000:
                if gpu_handle in [0x15678A00120000, 0x25678A00120000, 0x35678A00130000, 0x45678A00140000]:
                    print("   ⚠️  Fake GPU handle detected!")
                else:
                    print("   ✅ Valid GPU handle!")

    # 7. Создаём буфер
    print("\n7. Creating vertex buffer...")
    vertex_data = b'\x00\x00\x00\x00' * 36  # 3 вершины по 12 байт
    buffer = lib.create_buffer(device, len(vertex_data), None)

    if buffer == 0:
        print("   ❌ Failed to create buffer!")
    else:
        print(f"   ✅ Buffer created: 0x{buffer:X}")

        # Обновляем данные
        print("\n8. Updating buffer data...")
        success = lib.update_subresource(buffer, vertex_data, len(vertex_data))

        if success:
            print("   ✅ Buffer updated successfully!")
        else:
            print("   ❌ Failed to update buffer")

    # 9. Проверяем frame команды
    print("\n9. Testing frame commands...")

    result = lib.begin_frame()
    print(f"   begin_frame: {'✅' if result else '❌'}")

    if result:
        result = lib.end_frame()
        print(f"   end_frame: {'✅' if result else '❌'}")

        result = lib.wait_for_gpu()
        print(f"   wait_for_gpu: {'✅' if result else '❌'}")

    frame_index = lib.get_frame_index()
    print(f"   Frame index: {frame_index}")

    # 10. Размеры дескрипторов
    print("\n10. Descriptor sizes:")
    rtv_size = lib.get_rtv_descriptor_size()
    cbv_size = lib.get_cbv_srv_uav_descriptor_size()
    print(f"    RTV size: {rtv_size} bytes")
    print(f"    CBV/SRV/UAV size: {cbv_size} bytes")

    # 11. Очистка
    print("\n11. Cleaning up...")

    if heap != 0:
        lib.release_resource(heap)
        print("   Heap released")

    if buffer != 0:
        lib.release_resource(buffer)
        print("   Buffer released")

    if queue != 0:
        lib.release_resource(queue)
        print("   Queue released")

    if device != 0:
        lib.release_resource(device)
        print("   Device released")

    lib.force_cleanup()
    print("   Force cleanup done")

    print("\n" + "=" * 60)
    print("Test completed!")
    print("=" * 60)

    # Итог
    if is_warp:
        print("\n⚠️  NOTE: Running on WARP (software renderer)")
        print("   For hardware acceleration, install GPU drivers or")
        print("   run on a machine with dedicated graphics card.")
    else:
        print("\n✅ SUCCESS: Running on real GPU hardware!")
        print(f"   GPU: {gpu_name.decode('utf-8') if gpu_name else 'Unknown'}")


if __name__ == "__main__":
    main()