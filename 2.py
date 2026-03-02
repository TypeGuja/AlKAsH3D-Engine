# check_shaders.py
import os, ctypes
from alkash3d.graphics.utils import d3d12_wrapper as dx

# проверяем, что компилятор найден
try:
    ctypes.WinDLL("d3dcompiler_47.dll")
    print("d3dcompiler_47.dll найден")
except OSError as e:
    print("Не удалось загрузить d3dcompiler_47.dll:", e)
    raise SystemExit

vert = os.path.abspath("resources/shaders/forward_vert.hlsl")
frag = os.path.abspath("resources/shaders/forward_frag.hlsl")
print("Vertex shader:", vert)
print("Fragment shader:", frag)

# пробуем скомпилировать вручную через обёртку
vs = dx.compile_shader(vert, "VSMain", "vs_5_0")
ps = dx.compile_shader(frag, "PSMain", "ps_5_0")
print("VS blob:", hex(vs) if vs else None)
print("PS blob:", hex(ps) if ps else None)
