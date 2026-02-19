# debug_dx12.py
"""
Минимальный тест DirectX 12 для отладки
"""

import ctypes
import sys
import traceback

print("=" * 60)
print("DX12 DEBUG TEST")
print("=" * 60)
print(f"Python version: {sys.version}")
print(f"Platform: {sys.platform}")
print()

# Проверка импорта
try:
    print("1. Trying to import d3d12_wrapper...")
    from alkash3d.graphics.utils import d3d12_wrapper as dx
    print("   ✅ d3d12_wrapper imported successfully")
    print(f"   DLL path: {dx._lib_path}")
    print(f"   Functions loaded: {[f for f in dir(dx) if f.startswith('_') and not f.startswith('__')]}")
except Exception as e:
    print(f"   ❌ Failed to import d3d12_wrapper: {e}")
    traceback.print_exc()

print()

# Проверка создания устройства
try:
    print("2. Testing device creation...")
    device = dx.create_device()
    if device and device.value:
        print(f"   ✅ Device created: {hex(device.value)}")
    else:
        print("   ❌ Device creation failed - null pointer")
except Exception as e:
    print(f"   ❌ Device creation error: {e}")
    traceback.print_exc()

print()

# Проверка создания очереди команд
try:
    print("3. Testing command queue creation...")
    if 'device' in locals() and device and device.value:
        queue = dx.create_command_queue(device)
        if queue and queue.value:
            print(f"   ✅ Command queue created: {hex(queue.value)}")
        else:
            print("   ❌ Command queue creation failed - null pointer")
    else:
        print("   ⚠️  Skipping - no valid device")
except Exception as e:
    print(f"   ❌ Command queue error: {e}")
    traceback.print_exc()

print()

print("=" * 60)
print("Test complete")
print("=" * 60)

input("Press Enter to exit...")