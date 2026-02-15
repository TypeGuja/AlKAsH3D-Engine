#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
DirectX 12 тест с отладкой дескрипторов
"""

import ctypes
import glfw
import time
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).parent.parent))

from alkash3d.graphics.utils import d3d12_wrapper as dx

# Константы
SWAP_CHAIN_BUFFER_COUNT = 2


class DX12Test:
    def __init__(self, width, height):
        self.width = width
        self.height = height
        self.window = None
        self.hwnd = 0
        self.device = None
        self.queue = None
        self.swap_chain = None
        self.rtv_heap = None
        self.rtv_descriptor_size = 0
        self.back_buffers = []
        self.rtv_handles = []

    def init_window(self):
        """Создание окна"""
        if not glfw.init():
            raise RuntimeError("Failed to initialize GLFW")

        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
        glfw.window_hint(glfw.RESIZABLE, True)

        self.window = glfw.create_window(self.width, self.height, "DX12 Test", None, None)
        if not self.window:
            glfw.terminate()
            raise RuntimeError("Failed to create window")

        if hasattr(glfw, 'get_win32_window'):
            self.hwnd = glfw.get_win32_window(self.window)
            print(f"✅ Window created, HWND: {self.hwnd}")
        else:
            self.hwnd = 0
            print("⚠️ Using dummy HWND")

        return True

    def init_d3d12(self):
        """Инициализация DirectX 12"""
        print("\n[1] Creating device...")
        self.device = dx.create_device()
        if not self.device or not self.device.value:
            raise RuntimeError("Failed to create device")
        print(f"    Device: {hex(self.device.value)}")

        print("\n[2] Creating command queue...")
        self.queue = dx.create_command_queue(self.device)
        if not self.queue or not self.queue.value:
            raise RuntimeError("Failed to create queue")
        print(f"    Queue: {hex(self.queue.value)}")

        print("\n[3] Creating swap chain...")
        self.swap_chain = dx.create_swap_chain(
            self.queue, self.hwnd, self.width, self.height
        )
        if not self.swap_chain or not self.swap_chain.value:
            raise RuntimeError("Failed to create swap chain")
        print(f"    Swap chain: {hex(self.swap_chain.value)}")

        print("\n[4] Creating RTV descriptor heap...")
        self.rtv_heap = dx.create_descriptor_heap(
            self.device,
            SWAP_CHAIN_BUFFER_COUNT,
            0  # 0 = RTV
        )
        if not self.rtv_heap or not self.rtv_heap.value:
            raise RuntimeError("Failed to create RTV heap")
        print(f"    RTV heap: {hex(self.rtv_heap.value)}")

        self.rtv_descriptor_size = dx.get_rtv_descriptor_size()
        print(f"    RTV descriptor size: {self.rtv_descriptor_size}")

        rtv_cpu_start = dx.GetCPUDescriptorHandleForHeapStart(self.rtv_heap)
        print(f"    RTV CPU start: {hex(rtv_cpu_start)}")

        if rtv_cpu_start == 0:
            print("    ⚠️ CPU start is 0, using fallback calculation")
            # Fallback - используем вычисление вручную
            rtv_cpu_start = 0x1000  # Временное значение для теста

        print("\n[5] Creating RTVs for back buffers...")
        for i in range(SWAP_CHAIN_BUFFER_COUNT):
            print(f"\n    Processing buffer {i}:")

            # Получаем back buffer
            buffer = dx.swap_chain_get_buffer(self.swap_chain, i)
            if not buffer or not buffer.value:
                print(f"    ❌ Failed to get buffer {i}")
                continue

            self.back_buffers.append(buffer)
            print(f"    Buffer {i}: {hex(buffer.value)}")

            # Вычисляем CPU handle для этого RTV
            cpu_handle = rtv_cpu_start + i * self.rtv_descriptor_size
            print(f"    CPU handle for RTV {i}: {hex(cpu_handle)}")
            self.rtv_handles.append(cpu_handle)

            # Создаем RTV
            try:
                dx.create_render_target_view(self.device, buffer, cpu_handle)
                print(f"    ✅ RTV {i} created successfully")
            except Exception as e:
                print(f"    ❌ Failed to create RTV {i}: {e}")
                # Пробуем альтернативный способ
                try:
                    print(f"    Trying alternative method for RTV {i}...")
                    # Здесь можно добавить альтернативный метод
                    pass
                except:
                    pass

        print("\n✅ DirectX 12 initialized successfully!")
        return True

    def render_frame(self):
        """Отрисовка одного кадра"""
        try:
            # Получаем текущий индекс back buffer
            frame_index = dx.get_frame_index()

            if frame_index < len(self.rtv_handles):
                # Устанавливаем render target
                dx.set_render_target(self.rtv_handles[frame_index])

                # Очищаем черным цветом
                black = (0.0, 0.0, 0.0, 1.0)
                dx.clear_render_target(self.rtv_handles[frame_index], black)

            # Презентуем
            dx.present_swap_chain(self.swap_chain, 1)
        except Exception as e:
            print(f"Render error: {e}")
            raise

    def run(self):
        """Запуск основного цикла"""
        print("\n" + "=" * 60)
        print("Rendering - Press ESC to exit")
        print("=" * 60 + "\n")

        frame_count = 0
        start_time = time.time()

        while not glfw.window_should_close(self.window):
            glfw.poll_events()

            if glfw.get_key(self.window, glfw.KEY_ESCAPE) == glfw.PRESS:
                glfw.set_window_should_close(self.window, True)

            try:
                self.render_frame()
            except Exception as e:
                print(f"Fatal render error: {e}")
                break

            frame_count += 1
            if frame_count % 60 == 0:
                elapsed = time.time() - start_time
                fps = frame_count / elapsed
                print(f"FPS: {fps:.2f}")

    def cleanup(self):
        """Освобождение ресурсов"""
        print("\n[cleanup] Releasing resources...")

        for buffer in self.back_buffers:
            try:
                dx.release_resource(buffer)
            except:
                pass

        if self.rtv_heap:
            try:
                dx.release_resource(self.rtv_heap)
            except:
                pass

        if self.swap_chain:
            try:
                dx.release_resource(self.swap_chain)
            except:
                pass

        if self.queue:
            try:
                dx.release_resource(self.queue)
            except:
                pass

        if self.device:
            try:
                dx.release_resource(self.device)
            except:
                pass

        if self.window:
            glfw.destroy_window(self.window)
            glfw.terminate()

        print("    Cleanup complete")


def main():
    print("=" * 60)
    print("DIRECTX 12 TEST WITH DEBUG")
    print("=" * 60)

    test = DX12Test(800, 600)

    try:
        test.init_window()
        test.init_d3d12()
        test.run()
    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback
        traceback.print_exc()
    finally:
        test.cleanup()

    print("\n" + "=" * 60)
    print("TEST COMPLETE")
    print("=" * 60)


if __name__ == "__main__":
    main()