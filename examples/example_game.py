#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
example_game.py - ИСПРАВЛЕННАЯ РАБОЧАЯ ВЕРСИЯ
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

        # Флаг для отслеживания состояния
        self.device_ok = False
        self.frame_index = 0

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

            self.frame_index = self.backend.get_frame_index()
            self.device_ok = True
            return True

        except Exception as e:
            print(f"❌ Ошибка: {e}")
            import traceback
            traceback.print_exc()
            return False

    def render_frame(self):
        """Отрисовка одного кадра."""
        if not self.device_ok:
            return False

        try:
            # Начало кадра - проверяем возвращаемое значение
            if not self.backend.begin_frame():
                print("❌ begin_frame failed")
                self.device_ok = False
                return False

            # Получаем текущий back buffer индекс
            self.frame_index = self.backend.get_frame_index()

            # Получаем правильный RTV для текущего back buffer
            if hasattr(self.backend, '_rtv_cpu_handles') and len(self.backend._rtv_cpu_handles) > self.frame_index:
                rtv = self.backend._rtv_cpu_handles[self.frame_index]
            else:
                # Fallback на первый RTV
                rtv = self.backend.rtv_heap.get_cpu_handle(0)
                print(f"⚠️ Using fallback RTV for index {self.frame_index}")

            # Устанавливаем render target
            if not self.backend.set_render_target(rtv):
                print("❌ set_render_target failed")
                return False

            # Очищаем цветом
            if not self.backend.clear_render_target(rtv, self.colors[self.color_index]):
                print("❌ clear_render_target failed")
                return False

            # Завершаем кадр
            if not self.backend.end_frame():
                print("❌ end_frame failed")
                return False

            return True

        except Exception as e:
            print(f"❌ Ошибка рендеринга: {e}")
            import traceback
            traceback.print_exc()
            self.device_ok = False
            return False

    def run(self):
        """Главный цикл."""
        if not self.init_device():
            return

        print("\n🟢 ЗАПУСК - цвет должен меняться каждую секунду!")
        print("🟢 Если видите цвет - все работает!\n")

        last_frame_time = time.time()
        frame_times = []
        frames_since_color_change = 0

        while not glfw.window_should_close(self.window):
            # Обработка событий
            glfw.poll_events()

            # Выход по ESC
            if glfw.get_key(self.window, glfw.KEY_ESCAPE) == glfw.PRESS:
                break

            # Проверяем состояние устройства
            if not self.device_ok:
                print("\n⚠️ Устройство потеряно, пытаемся пересоздать...")
                if not self.init_device():
                    print("❌ Не удалось пересоздать устройство")
                    break
                continue

            # Рендерим кадр
            frame_start = time.time()
            if not self.render_frame():
                print("\n❌ Ошибка рендеринга - ждем...")
                time.sleep(0.1)  # Пауза перед повторной попыткой
                continue

            # Презентуем кадр (sync_interval=1 для VSync)
            if not self.backend.present(sync_interval=1):
                print("❌ present failed")
                self.device_ok = False
                continue

            # Считаем время кадра
            frame_time = time.time() - frame_start
            frame_times.append(frame_time)
            if len(frame_times) > 60:
                frame_times.pop(0)

            self.frame_count += 1
            frames_since_color_change += 1

            # Меняем цвет каждые 60 кадров (~1 секунда при 60 FPS)
            if frames_since_color_change >= 60:
                self.color_index = (self.color_index + 1) % len(self.colors)
                frames_since_color_change = 0

                # Считаем FPS
                if frame_times:
                    avg_fps = 1.0 / (sum(frame_times) / len(frame_times))
                else:
                    avg_fps = 0

                color_names = ["КРАСНЫЙ", "ЗЕЛЕНЫЙ", "СИНИЙ", "ЖЕЛТЫЙ"]
                print(
                    f"\r🎨 Цвет: {color_names[self.color_index]} | FPS: {avg_fps:.1f} | Кадров: {self.frame_count} | BackBuffer: {self.frame_index}",
                    end="")
                self.frame_count = 0

            # Небольшая задержка для уменьшения нагрузки CPU
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
        app = WorkingDX12App()
        app.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Прервано")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback

        traceback.print_exc()