# test_minimal_shader.py
"""
Минимальный тест с шейдерами
"""

import sys
import os
import time
import numpy as np
import glfw
import ctypes

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from alkash3d.graphics import select_backend

# ==================== МИНИМАЛЬНЫЕ ШЕЙДЕРЫ ====================

VERTEX_SHADER_CODE = """
struct VSInput {
    float3 position : POSITION;
};

struct VSOutput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

VSOutput main(VSInput input) {
    VSOutput output;
    output.position = float4(input.position, 1.0);
    output.color = float4(1.0, 0.0, 0.0, 1.0); // Красный
    return output;
}
"""

PIXEL_SHADER_CODE = """
struct PSInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

float4 main(PSInput input) : SV_TARGET {
    return input.color;
}
"""


class ShaderManager:
    """Управление шейдерами"""

    def __init__(self, backend):
        self.backend = backend
        self.vs_blob = 0
        self.ps_blob = 0
        self.pso = None

        print("\n=== SHADER MANAGER ===")

        # Создаем временные файлы
        import tempfile

        self.vs_path = os.path.join(tempfile.gettempdir(), "minimal_vs.hlsl")
        with open(self.vs_path, "w") as f:
            f.write(VERTEX_SHADER_CODE)
        print(f"VS file: {self.vs_path}")

        self.ps_path = os.path.join(tempfile.gettempdir(), "minimal_ps.hlsl")
        with open(self.ps_path, "w") as f:
            f.write(PIXEL_SHADER_CODE)
        print(f"PS file: {self.ps_path}")

        # Компилируем
        self.compile()

    def compile(self):
        """Компиляция шейдеров"""
        print("\nCompiling vertex shader...")
        self.vs_blob = self.backend.compile_shader("vs", self.vs_path)
        print(f"VS blob: {hex(self.vs_blob)}")

        print("\nCompiling pixel shader...")
        self.ps_blob = self.backend.compile_shader("ps", self.ps_path)
        print(f"PS blob: {hex(self.ps_blob)}")

        # Проверяем результат
        if self.vs_blob and self.ps_blob and self.vs_blob != 0x12345678 and self.ps_blob != 0x12345678:
            print("\nCreating PSO...")
            vs_ptr = ctypes.c_void_p(self.vs_blob)
            ps_ptr = ctypes.c_void_p(self.ps_blob)
            self.pso = self.backend.create_graphics_ps(vs_ptr, ps_ptr)
            print(f"PSO: {self.pso}")
        else:
            print("\nShader compilation failed!")
            print("This usually means d3dcompiler_47.dll is missing")

    def use(self):
        """Активация шейдера"""
        if self.pso:
            return self.backend.set_graphics_pipeline(self.pso)
        return False

    def cleanup(self):
        """Очистка временных файлов"""
        try:
            os.unlink(self.vs_path)
            os.unlink(self.ps_path)
        except:
            pass


def create_triangle():
    """Создать треугольник"""
    vertices = np.array([
        # x     y     z
        -0.5, -0.5, 0.0,
        0.5, -0.5, 0.0,
        0.0, 0.5, 0.0,
    ], dtype=np.float32)
    return vertices


def main():
    print("=" * 60)
    print("MINIMAL SHADER TEST")
    print("=" * 60)

    # Инициализация GLFW
    if not glfw.init():
        print("Failed to init GLFW")
        return

    glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
    window = glfw.create_window(800, 600, "Minimal Shader Test", None, None)
    if not window:
        glfw.terminate()
        return

    # Захватываем курсор (необязательно)
    glfw.set_input_mode(window, glfw.CURSOR, glfw.CURSOR_NORMAL)

    # Создаем бэкенд
    print("\n[1] Creating backend...")
    backend = select_backend("dx12")

    # Инициализируем устройство
    hwnd = glfw.get_win32_window(window)
    print(f"  HWND: {hex(hwnd)}")

    print("\n[2] Initializing device...")
    backend.init_device(hwnd, 800, 600)

    # Создаем шейдеры
    print("\n[3] Creating shaders...")
    shader = ShaderManager(backend)

    # Создаем треугольник
    print("\n[4] Creating triangle...")
    vertices = create_triangle()
    vb = backend.create_buffer(vertices.tobytes(), usage="vertex")
    print(f"  Vertex buffer: {hex(vb.value if vb else 0)}")

    print("\n[5] Starting render loop...")
    print("  Press ESC to exit")
    print("-" * 60)

    frame_count = 0
    last_time = time.time()

    while not glfw.window_should_close(window):
        glfw.poll_events()

        # Начинаем кадр
        if not backend.begin_frame():
            time.sleep(0.001)
            continue

        # Получаем back buffer
        if backend.rtv_heap and backend.rtv_heap.num_descriptors > 0:
            frame_idx = backend.get_frame_index() % 2
            back_rtv = backend.rtv_heap.get_cpu_handle(frame_idx)
            backend.set_render_target(back_rtv)

            # Очищаем экран темно-синим
            backend.clear_render_target(back_rtv, (0.2, 0.2, 0.3, 1.0))

        # Устанавливаем вьюпорт
        backend.set_viewport(0, 0, 800, 600)

        # Пробуем рисовать треугольник
        if shader.use():
            backend.set_vertex_buffers(vb, None)
            backend.draw(3)
            if frame_count % 60 == 0:
                print(f"  Drawing triangle at frame {frame_count}")
        else:
            if frame_count % 60 == 0:
                print(f"  Frame {frame_count} - no shader")

        # Завершаем кадр
        backend.end_frame()

        frame_count += 1

        # Проверяем ESC
        if glfw.get_key(window, 256) == glfw.PRESS:
            break

    print("\n[6] Shutting down...")
    shader.cleanup()
    backend.shutdown()
    glfw.terminate()
    print("Done")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n\nInterrupted")
    except Exception as e:
        print(f"\nError: {e}")
        import traceback

        traceback.print_exc()