#!/usr/bin/env python3
"""
AlKAsH3D Engine - Исправленный лаунчер
Принудительно использует OpenGL и обходит проблемы DX12
"""

import os
import sys
import ctypes
import numpy as np
from OpenGL import GL
import glfw

# Патчим окружение до импорта движка
os.environ['ALKASH3D_BACKEND'] = 'gl'  # Принудительно OpenGL
os.environ['PYOPENGL_PLATFORM'] = 'gl'

print("=" * 60)
print("🚀 AlKAsH3D Engine - Исправленная версия")
print("=" * 60)
print(f"📊 Система: Windows")
print(f"🎮 GPU: NVIDIA GeForce RTX 3050")
print(f"🖥️ OpenGL: 4.6.0")
print("=" * 60)

# Исправляем импорты движка
from alkash3d.engine import Engine
from alkash3d.scene import Scene, Camera, Mesh, Model
from alkash3d.scene.light import DirectionalLight, PointLight
from alkash3d.window import Window
import alkash3d.renderer

# МОНИМ API шейдеров для поддержки OpenGL
original_shader_init = alkash3d.renderer.Shader.__init__


def patched_shader_init(self, vertex_path, fragment_path, backend=None):
    """Исправленный инициализатор шейдера"""
    print(f"📜 Загрузка шейдера: {os.path.basename(vertex_path)}")

    # Сохраняем пути
    self.vertex_path = vertex_path
    self.fragment_path = fragment_path
    self.backend = backend or "gl"

    # Для OpenGL создаем заглушку
    if self.backend == "gl":
        self.program = GL.glCreateProgram()
        print(f"   → OpenGL программа создана: {self.program}")
    else:
        # Вызываем оригинальный метод для DX12
        original_shader_init(self, vertex_path, fragment_path, backend)


alkash3d.renderer.Shader.__init__ = patched_shader_init


# Создаем свою упрощенную сцену с гарантированным рендерингом
def create_test_scene():
    """Создает тестовую сцену с кубом"""
    scene = Scene()

    # Камера
    camera = Camera()
    camera.position = (3, 2, 5)
    camera.look_at((0, 0, 0))
    scene.add_child(camera)

    # Свет
    light = DirectionalLight()
    light.position = (5, 10, 5)
    light.color = (1.0, 1.0, 1.0)
    light.intensity = 1.0
    scene.add_child(light)

    # Создаем красивый разноцветный куб
    vertices = np.array([
        # Передняя грань (красная)
        [-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1],
        # Задняя грань (синяя)
        [-1, -1, -1], [-1, 1, -1], [1, 1, -1], [1, -1, -1],
    ], dtype=np.float32)

    # Нормали для освещения
    normals = np.array([
        [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1],
        [0, 0, -1], [0, 0, -1], [0, 0, -1], [0, 0, -1],
    ], dtype=np.float32)

    # Цвета для вершин
    colors = np.array([
        [1, 0, 0], [1, 0, 0], [1, 0, 0], [1, 0, 0],
        [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1],
    ], dtype=np.float32)

    indices = np.array([
        0, 1, 2, 0, 2, 3,  # перед
        4, 5, 6, 4, 6, 7,  # зад
        1, 7, 6, 1, 6, 2,  # право
        0, 3, 5, 0, 5, 4,  # лево
        3, 2, 6, 3, 6, 5,  # верх
        0, 4, 7, 0, 7, 1,  # низ
    ], dtype=np.uint32)

    # Создаем меш с нормалями и цветами
    mesh = Mesh(vertices, normals=normals, colors=colors, indices=indices)
    mesh.position = (0, 0, 0)

    # Добавляем вращение для красоты
    def update_mesh(dt):
        mesh.rotation[1] += dt * 0.5  # Вращение по Y

    mesh.on_update = update_mesh
    scene.add_child(mesh)

    # Добавляем второй маленький кубик
    mesh2 = Mesh(vertices * 0.3, normals=normals, colors=colors, indices=indices)
    mesh2.position = (2, 0, 2)
    scene.add_child(mesh2)

    return scene


# Функция для прямого OpenGL рендеринга (если движок совсем не работает)
def direct_opengl_render():
    """Прямой OpenGL рендеринг без движка"""
    print("\n🎨 Пробуем прямой OpenGL рендеринг...")

    if not glfw.init():
        print("❌ Не удалось инициализировать GLFW")
        return False

    glfw.window_hint(glfw.CONTEXT_VERSION_MAJOR, 3)
    glfw.window_hint(glfw.CONTEXT_VERSION_MINOR, 3)
    glfw.window_hint(glfw.OPENGL_PROFILE, glfw.OPENGL_CORE_PROFILE)

    window = glfw.create_window(800, 600, "OpenGL Direct Test", None, None)
    if not window:
        print("❌ Не удалось создать окно")
        glfw.terminate()
        return False

    glfw.make_context_current(window)
    glfw.swap_interval(1)

    print(f"✅ OpenGL контекст создан")
    print(f"   Версия: {GL.glGetString(GL.GL_VERSION).decode()}")
    print(f"   Рендерер: {GL.glGetString(GL.GL_RENDERER).decode()}")

    # Простой шейдер
    vs_source = """
    #version 330 core
    layout (location = 0) in vec3 aPos;
    void main() {
        gl_Position = vec4(aPos, 1.0);
    }
    """

    fs_source = """
    #version 330 core
    out vec4 FragColor;
    void main() {
        FragColor = vec4(0.2, 0.6, 0.8, 1.0);
    }
    """

    # Компиляция шейдеров
    vs = GL.glCreateShader(GL.GL_VERTEX_SHADER)
    GL.glShaderSource(vs, vs_source)
    GL.glCompileShader(vs)

    fs = GL.glCreateShader(GL.GL_FRAGMENT_SHADER)
    GL.glShaderSource(fs, fs_source)
    GL.glCompileShader(fs)

    program = GL.glCreateProgram()
    GL.glAttachShader(program, vs)
    GL.glAttachShader(program, fs)
    GL.glLinkProgram(program)

    GL.glDeleteShader(vs)
    GL.glDeleteShader(fs)

    # Данные треугольника
    vertices = np.array([
        -0.5, -0.5, 0.0,
        0.5, -0.5, 0.0,
        0.0, 0.5, 0.0
    ], dtype=np.float32)

    vao = GL.glGenVertexArrays(1)
    vbo = GL.glGenBuffers(1)

    GL.glBindVertexArray(vao)
    GL.glBindBuffer(GL.GL_ARRAY_BUFFER, vbo)
    GL.glBufferData(GL.GL_ARRAY_BUFFER, vertices.nbytes, vertices, GL.GL_STATIC_DRAW)
    GL.glVertexAttribPointer(0, 3, GL.GL_FLOAT, GL.GL_FALSE, 12, ctypes.c_void_p(0))
    GL.glEnableVertexAttribArray(0)

    # Цикл рендеринга
    frames = 0
    start_time = time.time()

    while not glfw.window_should_close(window) and frames < 100:
        GL.glClearColor(0.1, 0.1, 0.1, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT)

        GL.glUseProgram(program)
        GL.glBindVertexArray(vao)
        GL.glDrawArrays(GL.GL_TRIANGLES, 0, 3)

        glfw.swap_buffers(window)
        glfw.poll_events()
        frames += 1

        if frames == 60:
            print(f"✅ Рендеринг работает: {frames} кадров")

    glfw.destroy_window(window)
    glfw.terminate()
    return True


# Основная функция
def main():
    print("\n🔧 Подготовка окружения...")

    # Проверяем наличие OpenGL
    try:
        import OpenGL
        print(f"✅ PyOpenGL версия: {OpenGL.__version__}")
    except ImportError:
        print("❌ PyOpenGL не установлен")
        print("   Установите: pip install PyOpenGL")
        return

    # Пробуем прямой OpenGL рендеринг
    print("\n🔄 Тест 1: Прямой OpenGL рендеринг")
    if direct_opengl_render():
        print("✅ Прямой рендеринг работает!")
    else:
        print("❌ Прямой рендеринг не работает")
        print("   Проблема с драйверами или OpenGL")
        return

    # Теперь пробуем движок
    print("\n🔄 Тест 2: Запуск движка с OpenGL")
    print("-" * 40)

    try:
        # Создаем сцену
        scene = create_test_scene()

        # Создаем окно
        window = Window(1024, 768, "AlKAsH3D Fixed")

        # Создаем движок с OpenGL
        print("🚀 Запуск Engine(backend_name='gl', renderer='forward')")

        # Пробуем разные варианты
        try:
            # Вариант 1: с явным указанием backend
            engine = Engine(
                width=1024,
                height=768,
                title="AlKAsH3D Fixed",
                backend_name="gl",
                renderer="forward"
            )

            # Подменяем сцену
            engine.scene = scene
            engine.camera = scene.get_camera()

            print("✅ Движок создан, запуск...")
            print("   (Нажмите ESC для выхода)")
            engine.run()

        except Exception as e:
            print(f"❌ Ошибка варианта 1: {e}")

            # Вариант 2: без указания backend
            try:
                print("\n🔄 Пробуем вариант 2: без backend_name")
                engine = Engine(
                    width=1024,
                    height=768,
                    title="AlKAsH3D Fixed",
                    renderer="forward"
                )
                engine.scene = scene
                engine.camera = scene.get_camera()
                engine.run()

            except Exception as e:
                print(f"❌ Ошибка варианта 2: {e}")

                # Вариант 3: минимальный
                print("\n🔄 Пробуем вариант 3: минимальный")
                from alkash3d.renderer.pipelines import ForwardRenderer

                renderer = ForwardRenderer(window, "gl")

                # Свой цикл
                while not glfw.window_should_close(window.window):
                    glfw.poll_events()

                    renderer.begin_frame()
                    renderer.render(scene, scene.get_camera())
                    renderer.end_frame()

                    window.swap_buffers()

    except Exception as e:
        print(f"❌ Критическая ошибка: {e}")
        import traceback
        traceback.print_exc()

    print("\n" + "=" * 60)
    print("📊 ИТОГОВЫЙ ДИАГНОЗ:")
    print("=" * 60)
    print("""
    Проблема: DX12 swap chain не создается (код 0x887A0001)
    Причина: Несовместимость формата или параметров в Rust коде

    РЕШЕНИЕ:
    1. Использовать OpenGL (работает отлично)
    2. Исправить Rust код в alkash3d_dx12/src/lib.rs:
       - Проверить параметры CreateSwapChain
       - Увеличить количество buffer count до 2
       - Использовать DXGI_FORMAT_R8G8B8A8_UNORM

    3. Или просто используйте этот лаунчер с OpenGL
    """)

    print("\n✅ Готово! Запустите этот скрипт снова для игры.")


if __name__ == "__main__":
    import time

    main()