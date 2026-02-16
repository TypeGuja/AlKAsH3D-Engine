#!/usr/bin/env python3
"""
Тест отрисовки треугольника через DirectX 12
Исправленная версия
"""

import ctypes
import glfw
import numpy as np
import time
from pathlib import Path


# Цветной вывод для консоли
class Colors:
    HEADER = '\033[95m'
    BLUE = '\033[94m'
    GREEN = '\033[92m'
    WARNING = '\033[93m'
    FAIL = '\033[91m'
    ENDC = '\033[0m'
    BOLD = '\033[1m'


def print_header(text):
    print(f"\n{Colors.HEADER}{Colors.BOLD}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.HEADER}{Colors.BOLD} {text}{Colors.ENDC}")
    print(f"{Colors.HEADER}{Colors.BOLD}{'=' * 60}{Colors.ENDC}")


def print_success(text):
    print(f"{Colors.GREEN}✅ {text}{Colors.ENDC}")


def print_error(text):
    print(f"{Colors.FAIL}❌ {text}{Colors.ENDC}")


def print_info(text):
    print(f"{Colors.BLUE}ℹ️  {text}{Colors.ENDC}")


# Загружаем DLL
dll_path = Path("alkash3d/graphics/utils/alkash3d_dx12.dll")
if not dll_path.exists():
    dll_path = Path("alkash3d_dx12.dll")

print_header("ЗАГРУЗКА DLL")
print_info(f"Путь: {dll_path}")
dll = ctypes.CDLL(str(dll_path))
print_success("DLL загружена")

# Настройка типов для всех функций
print_header("НАСТРОЙКА ФУНКЦИЙ")

# Основные функции
dll.init_device.argtypes = [ctypes.c_size_t, ctypes.c_uint32, ctypes.c_uint32]
dll.init_device.restype = ctypes.c_bool

dll.begin_frame.argtypes = []
dll.begin_frame.restype = None

dll.end_frame.argtypes = []
dll.end_frame.restype = ctypes.c_bool

dll.cleanup.argtypes = []

dll.get_device.argtypes = []
dll.get_device.restype = ctypes.c_void_p

# Буферы
dll.create_buffer.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p]
dll.create_buffer.restype = ctypes.c_void_p

dll.update_subresource.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
dll.update_subresource.restype = None

# Рендеринг
dll.set_graphics_pipeline.argtypes = [ctypes.c_void_p]
dll.set_graphics_pipeline.restype = None

dll.set_vertex_buffers.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
dll.set_vertex_buffers.restype = None

dll.draw_indexed_instanced.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_int32,
                                       ctypes.c_uint32]
dll.draw_indexed_instanced.restype = None

# Шейдеры
dll.compile_shader.argtypes = [ctypes.c_wchar_p, ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)]
dll.compile_shader.restype = ctypes.c_int

dll.create_graphics_ps.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
dll.create_graphics_ps.restype = ctypes.c_void_p

# Вспомогательные
dll.get_frame_index.argtypes = []
dll.get_frame_index.restype = ctypes.c_uint32

print_success("Типы функций настроены")

# Создание окна
print_header("СОЗДАНИЕ ОКНА")
glfw.init()
glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
window = glfw.create_window(800, 600, "DX12 Triangle Test", None, None)
hwnd = glfw.get_win32_window(window)
print_info(f"HWND: 0x{hwnd:X}")
print_success("Окно создано")

# Очистка предыдущего состояния
print_header("ОЧИСТКА")
if hasattr(dll, 'cleanup'):
    dll.cleanup()
    print_success("Предыдущее состояние очищено")
time.sleep(0.1)

# Инициализация устройства
print_header("ИНИЦИАЛИЗАЦИЯ УСТРОЙСТВА")
if not dll.init_device(hwnd, 800, 600):
    print_error("Ошибка инициализации")
    glfw.terminate()
    exit(1)
print_success("Устройство инициализировано")

device_ptr = dll.get_device()
print_info(f"Указатель устройства: {device_ptr}")

# Создание шейдеров
print_header("КОМПИЛЯЦИЯ ШЕЙДЕРОВ")

# Вершинный шейдер (зеленый треугольник)
vs_code = """
struct VSInput {
    float3 position : POSITION;
};

struct VSOutput {
    float4 position : SV_POSITION;
};

VSOutput main(VSInput input) {
    VSOutput output;
    output.position = float4(input.position, 1.0);
    return output;
}
"""

# Фрагментный шейдер (зеленый цвет)
ps_code = """
struct PSInput {
    float4 position : SV_POSITION;
};

float4 main(PSInput input) : SV_TARGET {
    return float4(0.0, 1.0, 0.0, 1.0); // Зеленый
}
"""

# Сохраняем шейдеры во временные файлы
with open("temp_vs.hlsl", "w", encoding='utf-8') as f:
    f.write(vs_code)
with open("temp_ps.hlsl", "w", encoding='utf-8') as f:
    f.write(ps_code)

# Компилируем вершинный шейдер
vs_blob = ctypes.c_void_p()
result = dll.compile_shader("temp_vs.hlsl", b"main", b"vs_5_0", ctypes.byref(vs_blob))
if result == 0 and vs_blob.value:
    print_success(f"Вершинный шейдер скомпилирован: {vs_blob}")
else:
    print_error(f"Ошибка компиляции вершинного шейдера: {result}")

# Компилируем фрагментный шейдер
ps_blob = ctypes.c_void_p()
result = dll.compile_shader("temp_ps.hlsl", b"main", b"ps_5_0", ctypes.byref(ps_blob))
if result == 0 and ps_blob.value:
    print_success(f"Фрагментный шейдер скомпилирован: {ps_blob}")
else:
    print_error(f"Ошибка компиляции фрагментного шейдера: {result}")

# Создаем PSO (Pipeline State Object)
print_header("СОЗДАНИЕ PSO")
pso = dll.create_graphics_ps(device_ptr, vs_blob, ps_blob)
if pso:
    print_success(f"PSO создан: {pso}")
else:
    print_error("Ошибка создания PSO")

# Данные треугольника
print_header("СОЗДАНИЕ БУФЕРОВ")

# Вершины треугольника (большой, чтобы точно было видно)
vertices = np.array([
    -0.8, -0.8, 0.0,
    0.8, -0.8, 0.0,
    0.0, 0.8, 0.0
], dtype=np.float32)

indices = np.array([0, 1, 2], dtype=np.uint32)

print_info(f"Размер вершинных данных: {vertices.nbytes} байт")
print_info(f"Размер индексных данных: {indices.nbytes} байт")

# Создаем вершинный буфер
vb = dll.create_buffer(device_ptr, vertices.nbytes, b"vertex")
if vb:
    dll.update_subresource(vb, vertices.ctypes.data, vertices.nbytes)
    print_success(f"Вершинный буфер создан: {vb}")
else:
    print_error("Ошибка создания вершинного буфера")

# Создаем индексный буфер
ib = dll.create_buffer(device_ptr, indices.nbytes, b"index")
if ib:
    dll.update_subresource(ib, indices.ctypes.data, indices.nbytes)
    print_success(f"Индексный буфер создан: {ib}")
else:
    print_error("Ошибка создания индексного буфера")

# Главный цикл рендеринга
print_header("ЗАПУСК РЕНДЕРИНГА")
print_info("Нажмите ESC для выхода")
print_info("Должен появиться ЗЕЛЕНЫЙ треугольник на ЧЕРНОМ фоне")

frame = 0
while not glfw.window_should_close(window):
    glfw.poll_events()

    # Выход по ESC
    if glfw.get_key(window, glfw.KEY_ESCAPE) == glfw.PRESS:
        break

    # Начинаем новый кадр
    dll.begin_frame()

    # Устанавливаем PSO (шейдеры)
    if pso:
        dll.set_graphics_pipeline(pso)

    # Устанавливаем буферы и рисуем
    if vb and ib:
        dll.set_vertex_buffers(vb, ib)
        dll.draw_indexed_instanced(3, 1, 0, 0, 0)

    # Заканчиваем кадр (очистка экрана и отрисовка)
    dll.end_frame()

    frame += 1
    if frame % 60 == 0:
        print_info(f"Кадров отрисовано: {frame}")

    # Небольшая задержка для контроля FPS
    time.sleep(0.016)

# Очистка ресурсов
print_header("ОЧИСТКА РЕСУРСОВ")
dll.cleanup()
glfw.terminate()
print_success(f"Тест завершен! Всего кадров: {frame}")

# Удаляем временные файлы шейдеров
import os

try:
    os.remove("temp_vs.hlsl")
    os.remove("temp_ps.hlsl")
except:
    pass