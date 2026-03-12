#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
example_game_simple.py - МАКСИМАЛЬНО ПРОСТАЯ РАБОЧАЯ ВЕРСИЯ
"""

import sys
import time
import numpy as np
import ctypes
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import glfw
from alkash3d.graphics import select_backend


class SimpleApp:
    def __init__(self):
        print("\n" + "=" * 60)
        print("✅ DX12 SIMPLE - МИНИМАЛЬНАЯ РАБОЧАЯ ВЕРСИЯ")
        print("=" * 60)
        print("Просто меняет цвет фона")
        print("ESC - выход")
        print("=" * 60 + "\n")

        # Инициализация GLFW
        if not glfw.init():
            raise RuntimeError("Failed to init GLFW")

        # Создаем окно
        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
        self.window = glfw.create_window(800, 600, "DX12 Simple", None, None)
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

        self.device_ok = False
        self.frame_count = 0
        self.last_fps_time = time.time()

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

            if not hasattr(self.backend, '_rtv_cpu_handles') or len(self.backend._rtv_cpu_handles) < 2:
                print("❌ RTV handles не созданы!")
                return False

            print(f"✅ Swap chain: {hex(self.backend.swap_chain.value)}")
            print(f"✅ RTV handles: {len(self.backend._rtv_cpu_handles)}")

            self.device_ok = True
            return True

        except Exception as e:
            print(f"❌ Ошибка: {e}")
            import traceback
            traceback.print_exc()
            return False

    def render_frame(self):
        """Отрисовка одного кадра (только очистка)."""
        if not self.device_ok:
            return False

        try:
            # Начало кадра
            if not self.backend.begin_frame():
                return False

            # Получаем текущий back buffer индекс
            frame_idx = self.backend.get_frame_index()

            # Получаем RTV для текущего back buffer
            if frame_idx < len(self.backend._rtv_cpu_handles):
                rtv = self.backend._rtv_cpu_handles[frame_idx]
            else:
                rtv = self.backend._rtv_cpu_handles[0]

            # Устанавливаем render target
            if not self.backend.set_render_target(rtv):
                return False

            # Очищаем текущим цветом
            if not self.backend.clear_render_target(rtv, self.colors[self.color_index]):
                return False

            # Завершаем кадр
            if not self.backend.end_frame():
                return False

            return True

        except Exception as e:
            print(f"❌ Ошибка рендеринга: {e}")
            return False

    def run(self):
        """Главный цикл."""
        if not self.init_device():
            return

        print("\n🟢 ЗАПУСК - цвет меняется каждую секунду!")
        print("🟢 Если видите цвет - все работает!\n")

        last_color_change = time.time()

        while not glfw.window_should_close(self.window):
            # Обработка событий
            glfw.poll_events()

            # Выход по ESC
            if glfw.get_key(self.window, glfw.KEY_ESCAPE) == glfw.PRESS:
                break

            # Рендерим кадр
            if not self.render_frame():
                time.sleep(0.1)
                continue

            # Презентуем кадр
            if not self.backend.present(sync_interval=1):
                continue

            # Считаем FPS и меняем цвет
            self.frame_count += 1
            current_time = time.time()

            # Меняем цвет каждую секунду
            if current_time - last_color_change >= 1.0:
                self.color_index = (self.color_index + 1) % len(self.colors)

                # Считаем FPS
                fps = self.frame_count / (current_time - last_color_change)
                color_names = ["КРАСНЫЙ", "ЗЕЛЕНЫЙ", "СИНИЙ", "ЖЕЛТЫЙ"]
                print(f"\r🎨 {color_names[self.color_index]} | FPS: {fps:.1f}      ", end="")

                self.frame_count = 0
                last_color_change = current_time

            # Небольшая задержка
            time.sleep(0.001)

        self.cleanup()

    def cleanup(self):
        """Очистка."""
        print("\n\n👋 Завершение...")
        if hasattr(self, 'backend'):
            self.backend.shutdown()
        glfw.terminate()
        print("✅ Готово")


if __name__ == "__main__":
    try:
        app = SimpleApp()
        app.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Прервано")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback

        traceback.print_exc()