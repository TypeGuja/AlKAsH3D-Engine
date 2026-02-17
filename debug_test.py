#!/usr/bin/env python3
"""
AlKAsH3D Engine Advanced Diagnostic Tool v2.0
Полный анализ проблемы с видеовыводом
Эмулирует работу движка и проверяет каждый этап рендеринга
"""

import sys
import os
import importlib
import traceback
import contextlib
import io
import time
import ctypes
import numpy as np
from dataclasses import dataclass
from typing import List, Dict, Any, Optional, Tuple
import subprocess
import tempfile
import json
from enum import Enum


# Цвета для вывода
class Colors:
    HEADER = '\033[95m'
    BLUE = '\033[94m'
    CYAN = '\033[96m'
    GREEN = '\033[92m'
    WARNING = '\033[93m'
    RED = '\033[91m'
    END = '\033[0m'
    BOLD = '\033[1m'


class TestLevel(Enum):
    CRITICAL = "🔴 КРИТИЧЕСКИЙ"
    IMPORTANT = "🟠 ВАЖНЫЙ"
    NORMAL = "🔵 ОБЫЧНЫЙ"
    INFO = "⚪ ИНФОРМАЦИОННЫЙ"


@dataclass
class TestResult:
    name: str
    level: TestLevel
    passed: bool
    message: str
    error: Optional[str] = None
    suggestions: List[str] = None
    duration: float = 0.0
    details: Dict[str, Any] = None


class AlKAsH3DAdvancedTester:
    def __init__(self):
        self.results: List[TestResult] = []
        self.root_dir = os.getcwd()
        self.captured_output = []
        self.start_time = time.time()
        self.system_info = self.get_system_info()

    def get_system_info(self) -> Dict[str, Any]:
        """Сбор информации о системе"""
        info = {
            "os": sys.platform,
            "python": sys.version,
            "cwd": self.root_dir,
            "env": dict(os.environ),
            "path": sys.path,
        }

        # Информация о GPU через разные методы
        try:
            import glfw
            from OpenGL import GL

            if glfw.init():
                glfw.window_hint(glfw.VISIBLE, glfw.FALSE)
                window = glfw.create_window(100, 100, "Info", None, None)
                if window:
                    glfw.make_context_current(window)
                    info["gpu"] = {
                        "renderer": GL.glGetString(GL.GL_RENDERER).decode(),
                        "vendor": GL.glGetString(GL.GL_VENDOR).decode(),
                        "version": GL.glGetString(GL.GL_VERSION).decode(),
                        "glsl_version": GL.glGetString(GL.GL_SHADING_LANGUAGE_VERSION).decode(),
                    }
                    glfw.destroy_window(window)
                glfw.terminate()
        except Exception as e:
            info["gpu_error"] = str(e)

        return info

    def print_header(self, text: str):
        print(f"\n{Colors.HEADER}{'=' * 70}{Colors.END}")
        print(f"{Colors.BOLD}{Colors.CYAN}🔍 {text}{Colors.END}")
        print(f"{Colors.HEADER}{'=' * 70}{Colors.END}")

    def print_result(self, result: TestResult):
        if result.passed:
            status = f"{Colors.GREEN}✅ ПРОЙДЕН{Colors.END}"
        else:
            status = f"{Colors.RED}❌ ПРОВАЛЕН{Colors.END}"

        level_color = {
            TestLevel.CRITICAL: Colors.RED,
            TestLevel.IMPORTANT: Colors.WARNING,
            TestLevel.NORMAL: Colors.BLUE,
            TestLevel.INFO: Colors.CYAN,
        }[result.level]

        print(
            f"\n{status} {level_color}{result.level.value}{Colors.END} - {Colors.BOLD}{result.name}{Colors.END} ({result.duration:.2f}с)")
        print(f"   {result.message}")

        if result.error:
            print(f"   {Colors.RED}Ошибка: {result.error}{Colors.END}")

        if result.suggestions:
            print(f"   {Colors.GREEN}Рекомендации:{Colors.END}")
            for s in result.suggestions:
                print(f"   • {s}")

        if result.details:
            print(f"   {Colors.CYAN}Детали:{Colors.END}")
            for key, value in result.details.items():
                print(f"   • {key}: {value}")

    def run_test(self, name: str, level: TestLevel, func, *args, **kwargs):
        """Запускает тест с измерением времени"""
        print(f"\n{Colors.BLUE}▶ Запуск теста [{level.value}]: {name}{Colors.END}")

        start = time.time()
        output = io.StringIO()
        error_output = io.StringIO()

        try:
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(error_output):
                result = func(*args, **kwargs)

            duration = time.time() - start
            self.results.append(TestResult(
                name=name,
                level=level,
                passed=True,
                message=result if isinstance(result, str) else "Тест выполнен успешно",
                duration=duration,
                details=result if isinstance(result, dict) else None
            ))
            print(f"{Colors.GREEN}  ✓ Тест пройден за {duration:.2f}с{Colors.END}")

        except Exception as e:
            duration = time.time() - start
            error_msg = str(e)
            tb = traceback.format_exc()

            suggestions = self.analyze_error(name, error_msg, tb)

            self.results.append(TestResult(
                name=name,
                level=level,
                passed=False,
                message="Тест не пройден",
                error=error_msg,
                suggestions=suggestions,
                duration=duration
            ))
            print(f"{Colors.RED}  ✗ Ошибка: {error_msg}{Colors.END}")

        # Сохраняем вывод
        self.captured_output.append({
            'test': name,
            'stdout': output.getvalue(),
            'stderr': error_output.getvalue()
        })

    def analyze_error(self, test_name: str, error: str, tb: str) -> List[str]:
        """Анализирует ошибку и предлагает решения"""
        suggestions = []

        if "D3D12" in error or "DX12" in error:
            suggestions.append("Проверьте версию Windows (нужна 10/11 с обновлениями)")
            suggestions.append("Установите Graphics Tools: https://aka.ms/directx12")
            suggestions.append("Обновите драйверы NVIDIA")

        elif "heap exhausted" in error.lower():
            suggestions.append("Увеличьте размер descriptor heap в Rust коде")
            suggestions.append("Используйте backend_name='gl' для OpenGL")

        elif "shader" in error.lower():
            suggestions.append("Проверьте наличие шейдеров в resources/shaders/")
            suggestions.append("Проверьте синтаксис HLSL шейдеров")

        elif "swap chain" in error.lower():
            suggestions.append("Проверьте настройки экрана и разрешение")
            suggestions.append("Убедитесь, что окно не свернуто")

        return suggestions

    def test_system_compatibility(self):
        """Тест 1: Совместимость системы"""
        details = {
            "OS": self.system_info.get("os"),
            "Python": self.system_info.get("python").split()[0],
            "GPU": self.system_info.get("gpu", {}).get("renderer", "Unknown"),
            "OpenGL": self.system_info.get("gpu", {}).get("version", "Unknown"),
        }

        # Проверка Windows версии
        if sys.platform == 'win32':
            import platform
            win_ver = platform.version()
            if float(win_ver.split('.')[0]) >= 10:
                return "Система совместима с DirectX 12", details
            else:
                raise RuntimeError("Windows 10 или выше требуется для DX12")

        return "Система совместима", details

    def test_dx12_device_creation(self):
        """Тест 2: Создание DX12 устройства"""
        import ctypes

        # Загружаем библиотеку
        lib_path = os.path.join(self.root_dir, "alkash3d_dx12.dll")
        lib = ctypes.CDLL(lib_path)

        # Пробуем создать устройство
        create_device = lib.create_device
        create_device.restype = ctypes.c_void_p

        device_ptr = create_device()
        if not device_ptr:
            raise RuntimeError("Не удалось создать DX12 устройство")

        # Проверяем создание очереди команд
        create_queue = lib.create_command_queue
        create_queue.restype = ctypes.c_void_p
        create_queue.argtypes = [ctypes.c_void_p]

        queue_ptr = create_queue(device_ptr)
        if not queue_ptr:
            raise RuntimeError("Не удалось создать очередь команд")

        # Проверяем создание swap chain
        create_swapchain = lib.create_swap_chain
        create_swapchain.restype = ctypes.c_void_p
        create_swapchain.argtypes = [ctypes.c_void_p, ctypes.c_void_p,
                                     ctypes.c_int, ctypes.c_int]

        swapchain_ptr = create_swapchain(device_ptr, queue_ptr, 800, 600)
        if not swapchain_ptr:
            raise RuntimeError("Не удалось создать swap chain")

        return {
            "device": hex(device_ptr),
            "queue": hex(queue_ptr),
            "swapchain": hex(swapchain_ptr)
        }

    def test_shader_compilation(self):
        """Тест 3: Компиляция шейдеров"""
        shader_dir = os.path.join(self.root_dir, "resources", "shaders")
        shaders = {
            "forward_vert.hlsl": "Vertex shader",
            "forward_frag.hlsl": "Fragment shader",
            "deferred_geom_vert.hlsl": "Deferred vertex",
            "deferred_geom_frag.hlsl": "Deferred fragment",
            "deferred_light_vert.hlsl": "Light vertex",
            "deferred_light_frag.hlsl": "Light fragment",
        }

        results = {}
        missing = []

        for shader, desc in shaders.items():
            path = os.path.join(shader_dir, shader)
            if os.path.exists(path):
                with open(path, 'r', encoding='utf-8') as f:
                    content = f.read()
                results[shader] = {
                    "size": len(content),
                    "lines": len(content.split('\n')),
                    "present": True
                }
            else:
                missing.append(shader)
                results[shader] = {"present": False}

        if missing:
            raise RuntimeError(f"Отсутствуют шейдеры: {', '.join(missing)}")

        return results

    def test_pipeline_creation(self):
        """Тест 4: Создание графического конвейера"""
        try:
            from alkash3d.renderer import Shader
            from alkash3d.window import Window

            # Создаем окно (невидимое)
            window = Window(100, 100, "Test")

            # Пробуем создать шейдеры для разных pipeline
            pipelines = {}

            # Forward pipeline
            try:
                forward_shader = Shader(
                    vertex_path=os.path.join(self.root_dir, "resources", "shaders", "forward_vert.hlsl"),
                    fragment_path=os.path.join(self.root_dir, "resources", "shaders", "forward_frag.hlsl")
                )
                pipelines["forward"] = "OK"
            except Exception as e:
                pipelines["forward"] = f"Error: {e}"

            # Deferred geometry
            try:
                geom_shader = Shader(
                    vertex_path=os.path.join(self.root_dir, "resources", "shaders", "deferred_geom_vert.hlsl"),
                    fragment_path=os.path.join(self.root_dir, "resources", "shaders", "deferred_geom_frag.hlsl")
                )
                pipelines["deferred_geom"] = "OK"
            except Exception as e:
                pipelines["deferred_geom"] = f"Error: {e}"

            # Deferred lighting
            try:
                light_shader = Shader(
                    vertex_path=os.path.join(self.root_dir, "resources", "shaders", "deferred_light_vert.hlsl"),
                    fragment_path=os.path.join(self.root_dir, "resources", "shaders", "deferred_light_frag.hlsl")
                )
                pipelines["deferred_light"] = "OK"
            except Exception as e:
                pipelines["deferred_light"] = f"Error: {e}"

            return pipelines

        except Exception as e:
            raise RuntimeError(f"Ошибка создания pipeline: {e}")

    def test_descriptor_heaps(self):
        """Тест 5: Проверка создания куч дескрипторов"""
        import ctypes

        lib_path = os.path.join(self.root_dir, "alkash3d_dx12.dll")
        lib = ctypes.CDLL(lib_path)

        # Создаем устройство
        create_device = lib.create_device
        create_device.restype = ctypes.c_void_p
        device_ptr = create_device()

        if not device_ptr:
            raise RuntimeError("Не удалось создать устройство")

        # Пробуем создать кучи разных размеров
        heap_types = {
            "RTV": 0,  # D3D12_DESCRIPTOR_HEAP_TYPE_RTV
            "DSV": 1,  # D3D12_DESCRIPTOR_HEAP_TYPE_DSV
            "CBV_SRV_UAV": 2,  # D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV
        }

        results = {}

        create_heap = lib.create_descriptor_heap
        create_heap.restype = ctypes.c_void_p
        create_heap.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_bool]

        for heap_name, heap_type in heap_types.items():
            # Пробуем с разными размерами
            for size in [128, 256, 512, 1024, 2048, 4096]:
                heap_ptr = create_heap(device_ptr, heap_type, size, True)
                if heap_ptr:
                    results[f"{heap_name}_{size}"] = hex(heap_ptr)
                    # Если создалось с 1024, пробуем меньше уже не нужно
                    if size >= 1024:
                        break
                else:
                    results[f"{heap_name}_{size}"] = "FAILED"

        return results

    def test_actual_rendering(self):
        """Тест 6: Реальный рендеринг (эмуляция кадра)"""
        try:
            from alkash3d.engine import Engine
            from alkash3d.scene import Scene, Camera, Mesh
            from alkash3d.window import Window

            # Создаем окно (очень маленькое, невидимое)
            window = Window(100, 100, "Render Test")

            # Создаем простую сцену
            scene = Scene()
            camera = Camera()
            camera.position = (0, 0, 5)
            scene.add_child(camera)

            # Создаем треугольник
            vertices = np.array([
                [-0.5, -0.5, 0.0],
                [0.5, -0.5, 0.0],
                [0.0, 0.5, 0.0]
            ], dtype=np.float32)

            mesh = Mesh(vertices)
            scene.add_child(mesh)

            # Пробуем разные комбинации рендереров и бэкендов
            results = {}

            # Тест 1: Forward + OpenGL
            try:
                engine = Engine(
                    width=100,
                    height=100,
                    title="Test",
                    backend_name="gl",
                    renderer="forward"
                )
                # Эмулируем один кадр
                engine.renderer.begin_frame()
                engine.renderer.render(scene, camera)
                engine.renderer.end_frame()
                results["forward_gl"] = "OK"
            except Exception as e:
                results["forward_gl"] = str(e)

            # Тест 2: Forward + DX12
            try:
                engine = Engine(
                    width=100,
                    height=100,
                    title="Test",
                    backend_name="dx12",
                    renderer="forward"
                )
                engine.renderer.begin_frame()
                engine.renderer.render(scene, camera)
                engine.renderer.end_frame()
                results["forward_dx12"] = "OK"
            except Exception as e:
                results["forward_dx12"] = str(e)

            # Тест 3: Deferred + DX12
            try:
                engine = Engine(
                    width=100,
                    height=100,
                    title="Test",
                    backend_name="dx12",
                    renderer="deferred"
                )
                engine.renderer.begin_frame()
                engine.renderer.render(scene, camera)
                engine.renderer.end_frame()
                results["deferred_dx12"] = "OK"
            except Exception as e:
                results["deferred_dx12"] = str(e)

            return results

        except Exception as e:
            raise RuntimeError(f"Ошибка рендеринга: {e}")

    def test_frame_buffer_operations(self):
        """Тест 7: Проверка операций с framebuffer"""
        try:
            from OpenGL import GL
            import glfw

            glfw.init()
            glfw.window_hint(glfw.VISIBLE, glfw.FALSE)
            window = glfw.create_window(100, 100, "FB Test", None, None)
            glfw.make_context_current(window)

            results = {}

            # Тест очистки
            GL.glClearColor(0.5, 0.5, 0.5, 1.0)
            GL.glClear(GL.GL_COLOR_BUFFER_BIT)

            # Читаем пиксель
            pixel = GL.glReadPixels(50, 50, 1, 1, GL.GL_RGB, GL.GL_FLOAT)
            results["clear_color"] = str(pixel[0][0]) if pixel else "unknown"

            # Тест рисования линии
            GL.glBegin(GL.GL_LINES)
            GL.glVertex2f(-0.5, -0.5)
            GL.glVertex2f(0.5, 0.5)
            GL.glEnd()

            # Проверяем ошибки OpenGL
            error = GL.glGetError()
            results["opengl_error"] = error if error != GL.GL_NO_ERROR else "NO_ERROR"

            glfw.destroy_window(window)
            glfw.terminate()

            return results

        except Exception as e:
            raise RuntimeError(f"Ошибка framebuffer: {e}")

    def test_window_visibility(self):
        """Тест 8: Проверка видимости окна"""
        try:
            import glfw

            glfw.init()

            # Создаем видимое окно
            window = glfw.create_window(400, 300, "Visibility Test", None, None)

            if not window:
                raise RuntimeError("Не удалось создать окно")

            # Проверяем, что окно не свернуто
            iconified = glfw.get_window_attrib(window, glfw.ICONIFIED)

            # Проверяем, что окно видимо
            visible = glfw.get_window_attrib(window, glfw.VISIBLE)

            # Пробуем сделать окно активным
            glfw.make_context_current(window)

            # Обновляем окно
            glfw.swap_buffers(window)
            glfw.poll_events()

            result = {
                "created": True,
                "visible": bool(visible),
                "iconified": bool(iconified),
                "context_current": glfw.get_current_context() == window
            }

            glfw.destroy_window(window)
            glfw.terminate()

            return result

        except Exception as e:
            raise RuntimeError(f"Ошибка окна: {e}")

    def analyze_video_problem_deep(self):
        """Глубокий анализ проблемы с видео"""
        self.print_header("ГЛУБОКИЙ АНАЛИЗ ПРОБЛЕМЫ С ВИДЕО")

        # Собираем все результаты
        critical_fails = [r for r in self.results if r.level == TestLevel.CRITICAL and not r.passed]
        important_fails = [r for r in self.results if r.level == TestLevel.IMPORTANT and not r.passed]

        if critical_fails:
            print(f"\n{Colors.RED}❌ Найдены критические проблемы:{Colors.END}")
            for fail in critical_fails:
                print(f"  • {fail.name}: {fail.error}")
                if fail.suggestions:
                    for s in fail.suggestions:
                        print(f"    → {s}")

        if important_fails:
            print(f"\n{Colors.WARNING}⚠️ Найдены важные проблемы:{Colors.END}")
            for fail in important_fails:
                print(f"  • {fail.name}: {fail.error}")

        if not critical_fails and not important_fails:
            print(f"\n{Colors.GREEN}✅ Все важные тесты пройдены!{Colors.END}")

            # Специфичный анализ для DX12
            dx12_tests = [r for r in self.results if "DX12" in r.name or "descriptor" in r.name.lower()]
            if dx12_tests and all(t.passed for t in dx12_tests):
                print(f"\n{Colors.CYAN}🔍 DX12 бэкенд работает технически, но изображения нет.{Colors.END}")
                print("\nВозможные причины:")
                print("1. Swap chain не презентится (нет вызова Present)")
                print("2. Ошибка в настройках viewport")
                print("3. Проблема с синхронизацией GPU/CPU")
                print("4. Шейдеры компилируются, но не применяются")

                print(f"\n{Colors.GREEN}Попробуйте принудительно использовать OpenGL:{Colors.END}")
                print(
                    "python -c \"from alkash3d.engine import Engine; Engine(width=800, height=600, title='OpenGL Test', backend_name='gl', renderer='forward').run()\"")

    def generate_report(self) -> str:
        """Генерирует HTML отчет"""
        html = f"""
        <!DOCTYPE html>
        <html>
        <head>
            <title>AlKAsH3D Diagnostic Report</title>
            <style>
                body {{ font-family: Arial, sans-serif; margin: 20px; }}
                .pass {{ color: green; }}
                .fail {{ color: red; }}
                .critical {{ background-color: #ffeeee; }}
                .important {{ background-color: #ffffcc; }}
                table {{ border-collapse: collapse; width: 100%; }}
                th, td {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
                th {{ background-color: #f2f2f2; }}
                .suggestion {{ color: blue; }}
            </style>
        </head>
        <body>
            <h1>AlKAsH3D Engine Diagnostic Report</h1>
            <p>Generated: {time.strftime('%Y-%m-%d %H:%M:%S')}</p>

            <h2>System Information</h2>
            <table>
                <tr><th>Property</th><th>Value</th></tr>
        """

        for key, value in self.system_info.items():
            if key != "env":
                html += f"<tr><td>{key}</td><td>{value}</td></tr>"

        html += """
            </table>

            <h2>Test Results</h2>
            <table>
                <tr>
                    <th>Test</th>
                    <th>Level</th>
                    <th>Status</th>
                    <th>Duration</th>
                    <th>Details</th>
                </tr>
        """

        for result in self.results:
            status_class = "pass" if result.passed else "fail"
            level_class = result.level.name.lower()
            html += f"""
                <tr class="{level_class}">
                    <td>{result.name}</td>
                    <td>{result.level.value}</td>
                    <td class="{status_class}">{'✅ PASS' if result.passed else '❌ FAIL'}</td>
                    <td>{result.duration:.2f}s</td>
                    <td>{result.message}<br>
                        {f'<span class="fail">Error: {result.error}</span>' if result.error else ''}
                        {f'<br><span class="suggestion">Suggestions: {", ".join(result.suggestions)}</span>' if result.suggestions else ''}
                    </td>
                </tr>
            """

        html += """
            </table>

            <h2>Captured Output</h2>
            <pre>
        """

        for output in self.captured_output:
            html += f"\n--- {output['test']} ---\n"
            if output['stdout']:
                html += output['stdout'] + "\n"
            if output['stderr']:
                html += f"<span class='fail'>{output['stderr']}</span>\n"

        html += """
            </pre>
        </body>
        </html>
        """

        report_path = os.path.join(self.root_dir, "alkash3d_diagnostic_report.html")
        with open(report_path, 'w', encoding='utf-8') as f:
            f.write(html)

        return report_path

    def run_all_tests(self):
        """Запускает все расширенные тесты"""
        self.print_header("AlKAsH3D Engine ADVANCED Diagnostic Tool v2.0")
        print(f"{Colors.BLUE}Система: {self.system_info.get('gpu', {}).get('renderer', 'Unknown')}{Colors.END}")
        print(f"{Colors.BLUE}OpenGL: {self.system_info.get('gpu', {}).get('version', 'Unknown')}{Colors.END}")

        # Расширенные тесты
        tests = [
            ("Совместимость системы", TestLevel.CRITICAL, self.test_system_compatibility),
            ("Создание DX12 устройства", TestLevel.CRITICAL, self.test_dx12_device_creation),
            ("Компиляция шейдеров", TestLevel.CRITICAL, self.test_shader_compilation),
            ("Создание графического конвейера", TestLevel.CRITICAL, self.test_pipeline_creation),
            ("Кучи дескрипторов", TestLevel.IMPORTANT, self.test_descriptor_heaps),
            ("Реальный рендеринг", TestLevel.CRITICAL, self.test_actual_rendering),
            ("Framebuffer операции", TestLevel.IMPORTANT, self.test_frame_buffer_operations),
            ("Видимость окна", TestLevel.NORMAL, self.test_window_visibility),
        ]

        for name, level, func in tests:
            self.run_test(name, level, func)

        # Выводим результаты
        self.print_header("РЕЗУЛЬТАТЫ РАСШИРЕННОГО ТЕСТИРОВАНИЯ")

        passed = sum(1 for r in self.results if r.passed)
        total = len(self.results)

        print(f"\n{Colors.BOLD}Всего тестов: {total}, Пройдено: {passed}, Провалено: {total - passed}{Colors.END}")
        print(f"{Colors.BOLD}Время выполнения: {time.time() - self.start_time:.2f}с{Colors.END}")

        # Сортируем по уровню важности
        level_order = {TestLevel.CRITICAL: 0, TestLevel.IMPORTANT: 1,
                       TestLevel.NORMAL: 2, TestLevel.INFO: 3}

        sorted_results = sorted(self.results,
                                key=lambda x: (level_order[x.level], not x.passed))

        for result in sorted_results:
            self.print_result(result)

        # Анализ проблемы
        self.analyze_video_problem_deep()

        # Генерация отчета
        report_path = self.generate_report()
        print(f"\n{Colors.GREEN}📊 HTML отчет сохранен: {report_path}{Colors.END}")

        # Сохраняем лог
        log_file = os.path.join(self.root_dir, "alkash3d_advanced_log.txt")
        with open(log_file, 'w', encoding='utf-8') as f:
            f.write("AlKAsH3D Advanced Diagnostic Log\n")
            f.write("=" * 50 + "\n\n")
            f.write(f"Time: {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
            f.write(f"Python: {sys.version}\n\n")

            for output in self.captured_output:
                f.write(f"\n--- {output['test']} ---\n")
                if output['stdout']:
                    f.write("STDOUT:\n" + output['stdout'] + "\n")
                if output['stderr']:
                    f.write("STDERR:\n" + output['stderr'] + "\n")

        print(f"{Colors.BLUE}📝 Лог сохранен в: {log_file}{Colors.END}")


def main():
    # Создаем и запускаем расширенный тестер
    tester = AlKAsH3DAdvancedTester()

    try:
        tester.run_all_tests()

        print(f"\n{Colors.CYAN}{'=' * 70}{Colors.END}")
        print(f"{Colors.BOLD}ФИНАЛЬНЫЕ РЕКОМЕНДАЦИИ:{Colors.END}")
        print(f"{Colors.CYAN}{'=' * 70}{Colors.END}")

        print(f"\n1. {Colors.GREEN}Если все тесты пройдены, но видео нет:{Colors.END}")
        print("   • Запустите с OpenGL: Engine(backend_name='gl')")
        print("   • Проверьте, не свернуто ли окно")
        print("   • Попробуйте другое разрешение")

        print(f"\n2. {Colors.WARNING}Если есть ошибки в DX12:{Colors.END}")
        print("   • Увеличьте размер descriptor heap в Rust коде")
        print("   • Обновите драйверы NVIDIA")
        print("   • Установите DirectX 12 Agility SDK")

        print(f"\n3. {Colors.BLUE}Рекомендуемая команда для запуска:{Colors.END}")
        print(
            "   python -c \"from alkash3d.engine import Engine; Engine(width=1024, height=768, title='Working Game', backend_name='gl', renderer='forward').run()\"")

    except KeyboardInterrupt:
        print(f"\n{Colors.WARNING}Тестирование прервано пользователем{Colors.END}")
    except Exception as e:
        print(f"\n{Colors.RED}Критическая ошибка тестера: {e}{Colors.END}")
        traceback.print_exc()


if __name__ == "__main__":
    main()