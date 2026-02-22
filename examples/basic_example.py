#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
dx12_minimal_working.py - МИНИМАЛЬНАЯ РАБОЧАЯ ВЕРСИЯ
Просто показывает цветной экран через DX12
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import glfw
from alkash3d.graphics import select_backend


class MinimalDX12App:
    def __init__(self):
        print("\n" + "="*60)
        print("🚀 DX12 MINIMAL WORKING EXAMPLE")
        print("="*60)
        print("Если вы видите цветной экран - DX12 работает!")
        print("ESC - выход")
        print("="*60 + "\n")

        # Инициализация GLFW
        if not glfw.init():
            raise RuntimeError("Failed to init GLFW")

        # Создаем окно
        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
        self.window = glfw.create_window(800, 600, "DX12 Minimal", None, None)
        if not self.window:
            glfw.terminate()
            raise RuntimeError("Failed to create window")

        # Получаем HWND для DX12
        self.hwnd = glfw.get_win32_window(self.window)

        # Создаем DX12 бэкенд
        self.backend = select_backend("dx12")

        # Цвет для очистки (будем менять каждую секунду)
        self.colors = [
            (1.0, 0.2, 0.2, 1.0),  # Красный
            (0.2, 1.0, 0.2, 1.0),  # Зеленый
            (0.2, 0.2, 1.0, 1.0),  # Синий
            (1.0, 1.0, 0.2, 1.0),  # Желтый
            (1.0, 0.2, 1.0, 1.0),  # Пурпурный
            (0.2, 1.0, 1.0, 1.0),  # Голубой
        ]
        self.color_index = 0
        self.last_color_change = time.time()

        # Флаг для отслеживания инициализации
        self.initialized = False

    def init_device(self):
        """Инициализация устройства."""
        try:
            print("⏳ Инициализация DX12 устройства...")
            self.backend.init_device(self.hwnd, 800, 600)
            self.initialized = True
            print("✅ DX12 устройство инициализировано!")

            # Проверяем, создался ли swap chain
            if self.backend.swap_chain and self.backend.swap_chain.value:
                print(f"✅ Swap chain создан: {hex(self.backend.swap_chain.value)}")
            else:
                print("⚠️ Swap chain не создан - это нормально для теста")

        except Exception as e:
            print(f"❌ Ошибка инициализации: {e}")
            return False
        return True

    def run(self):
        """Главный цикл."""
        if not self.init_device():
            return

        print("\n🟢 Запуск цикла рендеринга...")
        print("🎨 Цвет должен меняться каждую секунду!\n")

        frame_count = 0
        fps_time = time.time()

        while not glfw.window_should_close(self.window):
            # Обработка событий
            glfw.poll_events()

            # Меняем цвет каждую секунду
            current_time = time.time()
            if current_time - self.last_color_change > 1.0:
                self.color_index = (self.color_index + 1) % len(self.colors)
                self.last_color_change = current_time
                color_name = ["Красный", "Зеленый", "Синий", "Желтый", "Пурпурный", "Голубой"][self.color_index]
                print(f"\rТекущий цвет: {color_name}", end="")

            # Рендеринг
            try:
                self.backend.begin_frame()

                if self.backend.rtv_heap and self.backend.rtv_heap.num_descriptors > 0:
                    # Берем первый RTV (даже если swap chain не создан)
                    rtv = self.backend.rtv_heap.get_cpu_handle(0)
                    self.backend.set_render_target(rtv)
                    self.backend.clear_render_target(rtv, self.colors[self.color_index])

                self.backend.end_frame()

            except Exception as e:
                print(f"\n❌ Ошибка рендеринга: {e}")

            # Счетчик FPS
            frame_count += 1
            if current_time - fps_time > 1.0:
                fps = frame_count
                frame_count = 0
                fps_time = current_time
                # Не выводим FPS каждую секунду, чтобы не засорять консоль

            # Выход по ESC
            if glfw.get_key(self.window, glfw.KEY_ESCAPE) == glfw.PRESS:
                break

        self.cleanup()

    def cleanup(self):
        """Очистка ресурсов."""
        print("\n\n👋 Завершение работы...")
        self.backend.shutdown()
        glfw.terminate()
        print("✅ Завершено!")


if __name__ == "__main__":
    try:
        app = MinimalDX12App()
        app.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Прервано пользователем")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback
        traceback.print_exc()