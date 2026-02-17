#!/usr/bin/env python3
"""
ФИНАЛЬНЫЙ ТЕСТ DX12 с очисткой
"""

import ctypes
import glfw
import numpy as np
from pathlib import Path

print("=" * 60)
print("ФИНАЛЬНЫЙ ТЕСТ DX12 С ОЧИСТКОЙ")
print("=" * 60)

# Загружаем DLL
dll_path = Path("alkash3d/graphics/utils/alkash3d_dx12.dll")
print(f"Загрузка: {dll_path}")
dll = ctypes.CDLL(str(dll_path))

# Настройка типов
dll.init_device.argtypes = [ctypes.c_size_t, ctypes.c_uint32, ctypes.c_uint32]
dll.init_device.restype = ctypes.c_bool

dll.begin_frame.argtypes = []
dll.begin_frame.restype = None

dll.set_graphics_pipeline.argtypes = [ctypes.c_void_p]
dll.set_graphics_pipeline.restype = None

dll.set_vertex_buffers.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
dll.set_vertex_buffers.restype = None

dll.draw_indexed_instanced.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_int32,
                                       ctypes.c_uint32]
dll.draw_indexed_instanced.restype = None

dll.end_frame.argtypes = []
dll.end_frame.restype = ctypes.c_bool

dll.cleanup.argtypes = []

dll.get_device.argtypes = []
dll.get_device.restype = ctypes.c_void_p

dll.create_buffer.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p]
dll.create_buffer.restype = ctypes.c_void_p

dll.update_subresource.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
dll.update_subresource.restype = None

dll.compile_shader.argtypes = [ctypes.c_wchar_p, ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)]
dll.compile_shader.restype = ctypes.c_int

dll.create_graphics_ps.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
dll.create_graphics_ps.restype = ctypes.c_void_p

# Создание окна
print("\nСоздание окна...")
glfw.init()
glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
window = glfw.create_window(800, 600, "DX12 Final", None, None)
hwnd = glfw.get_win32_window(window)
print(f"✓ HWND: 0x{hwnd:X}")

# ЯВНО ОЧИЩАЕМ СОСТОЯНИЕ ПЕРЕД ИНИЦИАЛИЗАЦИЕЙ
if hasattr(dll, 'cleanup'):
    print("\nОчистка предыдущего состояния...")
    dll.cleanup()

# Инициализация
print("\nИнициализация устройства...")
if not dll.init_device(hwnd, 800, 600):
    print("❌ Ошибка инициализации")
    glfw.terminate()
    exit(1)
print("✓ Устройство инициализировано")

# Получаем устройство
device_ptr = dll.get_device()
print(f"✓ Устройство: {device_ptr}")

# Теперь СОЗДАЕМ СВОИ буферы и шейдеры
print("\nСоздание своих буферов...")

vertices = np.array([
    -0.8, -0.8, 0.0,
    0.8, -0.8, 0.0,
    0.0, 0.8, 0.0
], dtype=np.float32)

indices = np.array([0, 1, 2], dtype=np.uint32)

vb = dll.create_buffer(device_ptr, vertices.nbytes, b"vertex")
print(f"✓ Вершинный буфер: {vb}")
dll.update_subresource(vb, vertices.ctypes.data, vertices.nbytes)

ib = dll.create_buffer(device_ptr, indices.nbytes, b"index")
print(f"✓ Индексный буфер: {ib}")
dll.update_subresource(ib, indices.ctypes.data, indices.nbytes)

print("\nКомпиляция своих шейдеров...")

vs_blob = ctypes.c_void_p()
result = dll.compile_shader(
    "shaders/simple_vs.hlsl",
    b"main",
    b"vs_5_0",
    ctypes.byref(vs_blob)
)
print(f"✓ Вершинный шейдер: {vs_blob} (result={result})")

ps_blob = ctypes.c_void_p()
result = dll.compile_shader(
    "shaders/simple_ps.hlsl",
    b"main",
    b"ps_5_0",
    ctypes.byref(ps_blob)
)
print(f"✓ Фрагментный шейдер: {ps_blob} (result={result})")

print("\nСоздание своего pipeline state...")
pso = dll.create_graphics_ps(device_ptr, vs_blob, ps_blob)
print(f"✓ PSO: {pso}")

print("\n" + "=" * 60)
print("ЗАПУСК РЕНДЕРИНГА")
print("=" * 60)

frame = 0
while not glfw.window_should_close(window) and frame < 100:
    glfw.poll_events()

    dll.begin_frame()

    # Устанавливаем свои ресурсы
    if pso:
        dll.set_graphics_pipeline(pso)
    if vb and ib:
        dll.set_vertex_buffers(vb, ib)
        dll.draw_indexed_instanced(3, 1, 0, 0, 0)

    dll.end_frame()

    frame += 1
    if frame % 10 == 0:
        print(f"✓ Кадров: {frame}")

print("\nОчистка...")
dll.cleanup()
glfw.terminate()
print(f"✓ Готово! Всего кадров: {frame}")