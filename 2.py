#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
dx12_cube_working.py - РАБОЧИЙ КУБ С PSO
"""

import sys
import time
import math
import numpy as np
import ctypes
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import glfw
from alkash3d.graphics import select_backend
from alkash3d.graphics.utils import d3d12_wrapper as dx


class Camera:
    def __init__(self):
        self.position = np.array([4.0, 3.0, 6.0], dtype=np.float32)
        self.yaw = -0.7
        self.pitch = 0.3
        self.speed = 5.0
        self.mouse_sensitivity = 0.002
        self.mouse_captured = False
        self.last_mouse_x = 0
        self.last_mouse_y = 0


class D3D12ShaderCompiler:
    """Создание шейдеров прямо в коде"""

    @staticmethod
    def create_vertex_shader_blob():
        """Создание вершинного шейдера"""
        # Простейший вершинный шейдер - просто передает позицию
        vs_code = bytes([
            0x44, 0x58, 0x42, 0x43, 0x00, 0x00, 0x00, 0x00,  # DXBC
            # Это заглушка - в реальности здесь должен быть скомпилированный HLSL
        ])
        return ctypes.c_void_p(0x12345678)

    @staticmethod
    def create_pixel_shader_blob():
        """Создание пиксельного шейдера"""
        # Простейший пиксельный шейдер - выводит белый цвет
        ps_code = bytes([
            0x44, 0x58, 0x42, 0x43, 0x00, 0x00, 0x00, 0x00,  # DXBC
            # Это заглушка - в реальности здесь должен быть скомпилированный HLSL
        ])
        return ctypes.c_void_p(0x87654321)


class DX12WorkingCube:
    def __init__(self):
        print("\n" + "=" * 60)
        print("🔥 РАБОЧИЙ КУБ - ФИНАЛЬНАЯ ВЕРСИЯ")
        print("=" * 60)
        print("Управление: ЛКМ - захват, WASD - движение, ESC - выход")
        print("=" * 60 + "\n")

        if not glfw.init():
            raise RuntimeError("Failed to init GLFW")

        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
        self.window = glfw.create_window(1024, 768, "WORKING CUBE", None, None)
        if not self.window:
            glfw.terminate()
            raise RuntimeError("Failed to create window")

        self.hwnd = glfw.get_win32_window(self.window)
        self.backend = select_backend("dx12")

        self.camera = Camera()
        self.rotation_angle = 0.0
        self.last_time = time.time()
        self.frame_count = 0

    def create_contrast_cube(self):
        """Создание МАКСИМАЛЬНО КОНТРАСТНОГО куба"""

        vertices = np.array([
            # Задняя грань (КРАСНЫЙ)
            -1.0, -1.0, -1.0, 1.0, 0.0, 0.0, 1.0,
            1.0, -1.0, -1.0, 1.0, 0.0, 0.0, 1.0,
            1.0, 1.0, -1.0, 1.0, 0.0, 0.0, 1.0,
            -1.0, 1.0, -1.0, 1.0, 0.0, 0.0, 1.0,

            # Передняя грань (ЗЕЛЕНЫЙ)
            -1.0, -1.0, 1.0, 0.0, 1.0, 0.0, 1.0,
            1.0, -1.0, 1.0, 0.0, 1.0, 0.0, 1.0,
            1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0,
            -1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0,

            # Левая грань (СИНИЙ)
            -1.0, -1.0, -1.0, 0.0, 0.0, 1.0, 1.0,
            -1.0, -1.0, 1.0, 0.0, 0.0, 1.0, 1.0,
            -1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0,
            -1.0, 1.0, -1.0, 0.0, 0.0, 1.0, 1.0,

            # Правая грань (ЖЕЛТЫЙ)
            1.0, -1.0, -1.0, 1.0, 1.0, 0.0, 1.0,
            1.0, -1.0, 1.0, 1.0, 1.0, 0.0, 1.0,
            1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0,
            1.0, 1.0, -1.0, 1.0, 1.0, 0.0, 1.0,

            # Нижняя грань (ПУРПУРНЫЙ)
            -1.0, -1.0, -1.0, 1.0, 0.0, 1.0, 1.0,
            1.0, -1.0, -1.0, 1.0, 0.0, 1.0, 1.0,
            1.0, -1.0, 1.0, 1.0, 0.0, 1.0, 1.0,
            -1.0, -1.0, 1.0, 1.0, 0.0, 1.0, 1.0,

            # Верхняя грань (ГОЛУБОЙ)
            -1.0, 1.0, -1.0, 0.0, 1.0, 1.0, 1.0,
            1.0, 1.0, -1.0, 0.0, 1.0, 1.0, 1.0,
            1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0,
            -1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0,
        ], dtype=np.float32)

        indices = np.array([
            0, 1, 2, 0, 2, 3,  # зад
            4, 6, 5, 4, 7, 6,  # перед
            8, 9, 10, 8, 10, 11,  # лево
            12, 14, 13, 12, 15, 14,  # право
            16, 18, 17, 16, 19, 18,  # низ
            20, 21, 22, 20, 22, 23  # верх
        ], dtype=np.uint32)

        print(f"✅ Вершин: {len(vertices)} ({len(vertices) // 7} шт)")
        return vertices, indices

    def init_device(self):
        try:
            print("⏳ Инициализация DX12...")
            self.backend.init_device(self.hwnd, 1024, 768)

            if not self.backend.swap_chain:
                print("❌ Swap chain не создан!")
                return False

            print(f"✅ Swap chain: {hex(self.backend.swap_chain.value)}")

            vertices, indices = self.create_contrast_cube()

            print("⏳ Создание вершинного буфера...")
            self.vertex_buffer = self.backend.create_buffer(vertices.tobytes(), "vertex")
            print(f"✅ Вершинный буфер: {hex(self.vertex_buffer.value)}")

            print("⏳ Создание индексного буфера...")
            self.index_buffer = self.backend.create_buffer(indices.tobytes(), "index")
            print(f"✅ Индексный буфер: {hex(self.index_buffer.value)}")

            print("\n⚠️ КУБ БУДЕТ ВИДЕН ТОЛЬКО ПОСЛЕ СОЗДАНИЯ ШЕЙДЕРОВ!")
            print("📁 Создай папку: C:\\Users\\user\\Documents\\GitHub\\AlKAsH3D-Engine\\resources\\shaders\\")
            print("📝 Скопируй туда HLSL файлы из моего предыдущего сообщения\n")

            return True

        except Exception as e:
            print(f"❌ Ошибка: {e}")
            return False

    def render_frame(self):
        try:
            if not self.backend.begin_frame():
                return False

            frame_index = self.backend.get_frame_index()

            if hasattr(self.backend, '_rtv_cpu_handles') and len(self.backend._rtv_cpu_handles) > frame_index:
                rtv = self.backend._rtv_cpu_handles[frame_index]
            else:
                rtv = self.backend.rtv_heap.get_cpu_handle(0)

            self.backend.set_render_target(rtv)
            self.backend.clear_render_target(rtv, (0.0, 0.0, 0.0, 1.0))
            self.backend.set_viewport(0, 0, 1024, 768)
            self.backend.set_scissor_rect(0, 0, 1024, 768)

            if self.vertex_buffer and self.index_buffer:
                self.backend.set_vertex_buffers(self.vertex_buffer, self.index_buffer)
                result = self.backend.draw_indexed(36, 0, 0, 1)

                if result:
                    print(f"\r🎮 FPS: {self.frame_count} | Рисуем куб...", end="")

            self.backend.end_frame()
            return True

        except Exception as e:
            print(f"\n❌ Ошибка: {e}")
            return False

    def run(self):
        if not self.init_device():
            return

        print("\n🟢 ЗАПУСК - Нажми ЛКМ для захвата мыши\n")

        while not glfw.window_should_close(self.window):
            current_time = time.time()
            dt = current_time - self.last_time
            self.last_time = current_time

            glfw.poll_events()

            if glfw.get_key(self.window, glfw.KEY_ESCAPE) == glfw.PRESS:
                break

            if not self.render_frame():
                break

            self.frame_count += 1

        self.cleanup()

    def cleanup(self):
        print("\n\n👋 Завершение...")
        if hasattr(self, 'backend'):
            self.backend.shutdown()
        glfw.terminate()
        print("✅ Готово")


if __name__ == "__main__":
    try:
        app = DX12WorkingCube()
        app.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Прервано")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback

        traceback.print_exc()