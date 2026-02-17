import inspect
from alkash3d.graphics.utils import *

print("=" * 60)
print("ПРОВЕРКА ФУНКЦИЙ В d3d12_wrapper")
print("=" * 60)

functions = [name for name in dir(dx) if not name.startswith('_')]
print(f"Найдено функций: {len(functions)}")

if 'begin_frame' in functions:
    print("✅ begin_frame найдена")
else:
    print("❌ begin_frame НЕ найдена")

if 'end_frame' in functions:
    print("✅ end_frame найдена")
else:
    print("❌ end_frame НЕ найдена")

if 'get_frame_index' in functions:
    print("✅ get_frame_index найдена")
else:
    print("❌ get_frame_index НЕ найдена")

print("\nПервые 20 функций:")
for f in functions[:20]:
    print(f"  - {f}")