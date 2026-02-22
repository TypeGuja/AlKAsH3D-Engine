# debug_check.py
import inspect
from alkash3d.graphics import DX12Backend
from alkash3d.graphics.backend import GraphicsBackend

print("=" * 60)
print("DEBUG: Checking DX12Backend implementation")
print("=" * 60)

# Получаем все абстрактные методы из базового класса
abstract_methods = []
for name, method in inspect.getmembers(GraphicsBackend, inspect.isfunction):
    if hasattr(method, '__isabstractmethod__') and method.__isabstractmethod__:
        abstract_methods.append(name)

print(f"\nAbstract methods in GraphicsBackend: {abstract_methods}")

# Проверяем, какие методы реализованы в DX12Backend
print("\nChecking DX12Backend methods:")
missing_methods = []

for method_name in abstract_methods:
    if hasattr(DX12Backend, method_name):
        method = getattr(DX12Backend, method_name)
        if not hasattr(method, '__isabstractmethod__') or not method.__isabstractmethod__:
            print(f"  ✅ {method_name} - implemented")
        else:
            print(f"  ❌ {method_name} - still abstract")
            missing_methods.append(method_name)
    else:
        print(f"  ❌ {method_name} - missing")
        missing_methods.append(method_name)

print(f"\nMissing methods: {missing_methods}")

# Проверяем сам файл
print("\nChecking file location:")
import alkash3d.graphics.dx12_backend
print(f"  Module: {alkash3d.graphics.dx12_backend.__file__}")

# Пытаемся создать экземпляр
print("\nAttempting to create instance:")
try:
    backend = DX12Backend()
    print("  ✅ Success!")
except Exception as e:
    print(f"  ❌ Failed: {e}")