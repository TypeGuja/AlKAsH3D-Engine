#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
dx12_working_final.py - ИСПРАВЛЕННАЯ РАБОЧАЯ ВЕРСИЯ
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import glfw
from alkash3d.graphics import select_backend


class WorkingDX12App:
    def __init__(self):
        print("\n" + "=" * 60)
        print("✅ DX12 WORKING FINAL - ДОЛЖНО РАБОТАТЬ")
        print("=" * 60)
        print("Если вы видите цветной экран - DX12 работает!")
        print("ESC - выход")
        print("=" * 60 + "\n")

        # Инициализация GLFW
        if not glfw.init():
            raise RuntimeError("Failed to init GLFW")

        # Создаем окно
        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
        self.window = glfw.create_window(800, 600, "DX12 Working", None, None)
        if not self.window:
            glfw.terminate()
            raise RuntimeError("Failed to create window")

        self.hwnd = glfw.get_win32_window(self.window)
        self.backend = select_backend("dx12")

        # Цвета для анимации
        self.colors = [
            (1.0, 0.0, 0.0, 1.0),  # Красный
            (0.0, 1.0, 0.0, 1.0),  # Зеленый
            (0.0, 0.0, 1.0, 1.0),  # Синий
            (1.0, 1.0, 0.0, 1.0),  # Желтый
        ]
        self.color_index = 0
        self.last_change = time.time()
        self.frame_count = 0
        self.fps_time = time.time()

    def init_device(self):
        """Инициализация устройства."""
        try:
            print("⏳ Инициализация DX12...")
            self.backend.init_device(self.hwnd, 800, 600)

            # Проверяем, что все создалось
            if not self.backend.swap_chain:
                print("❌ Swap chain не создан!")
                return False

            if not self.backend.rtv_heap:
                print("❌ RTV heap не создан!")
                return False

            print(f"✅ Swap chain: {hex(self.backend.swap_chain.value)}")
            print(f"✅ RTV heap: {self.backend.rtv_heap}")
            print(f"✅ RTV дескрипторов: {len(self.backend._rtv_cpu_handles)}")

            return True

        except Exception as e:
            print(f"❌ Ошибка: {e}")
            import traceback
            traceback.print_exc()
            return False

    def render_frame(self):
        """Отрисовка одного кадра."""
        try:
            # Начало кадра
            self.backend.begin_frame()

            # Получаем текущий back buffer индекс
            frame_index = self.backend.get_frame_index()

            # Получаем правильный RTV для текущего back buffer
            if hasattr(self.backend, '_rtv_cpu_handles') and len(self.backend._rtv_cpu_handles) > frame_index:
                rtv = self.backend._rtv_cpu_handles[frame_index]
            else:
                # Fallback на первый RTV
                rtv = self.backend.rtv_heap.get_cpu_handle(0)

            # Устанавливаем render target
            self.backend.set_render_target(rtv)

            # Очищаем цветом
            self.backend.clear_render_target(rtv, self.colors[self.color_index])

            # Завершаем кадр
            self.backend.end_frame()

            return True

        except Exception as e:
            print(f"❌ Ошибка рендеринга: {e}")
            return False

    def run(self):
        """Главный цикл."""
        if not self.init_device():
            return

        print("\n🟢 ЗАПУСК - цвет должен меняться каждую секунду!")
        print("🟢 Если видите цвет - все работает!\n")

        while not glfw.window_should_close(self.window):
            # Обработка событий
            glfw.poll_events()

            # Меняем цвет каждую секунду
            now = time.time()
            if now - self.last_change > 1.0:
                self.color_index = (self.color_index + 1) % len(self.colors)
                self.last_change = now
                color_names = ["КРАСНЫЙ", "ЗЕЛЕНЫЙ", "СИНИЙ", "ЖЕЛТЫЙ"]
                print(f"\r🎨 Цвет: {color_names[self.color_index]} | FPS: {self.frame_count}", end="")
                self.frame_count = 0

            # Рендерим кадр
            if not self.render_frame():
                print("\n❌ Ошибка рендеринга - останов")
                break

            # Счетчик FPS
            self.frame_count += 1

            # Выход по ESC
            if glfw.get_key(self.window, glfw.KEY_ESCAPE) == glfw.PRESS:
                break

        self.cleanup()

    def cleanup(self):
        """Очистка."""
        print("\n\n👋 Завершение...")
        self.backend.shutdown()
        glfw.terminate()
        print("✅ Готово")


if __name__ == "__main__":
    try:
        app = WorkingDX12App()
        app.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Прервано")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback

        traceback.print_exc()