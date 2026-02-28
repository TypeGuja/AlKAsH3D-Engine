import sys
import os
import inspect
import pkgutil
import importlib
from pathlib import Path

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))


def print_header(text):
    """Печать заголовка"""
    print("\n" + "=" * 80)
    print(f" {text}")
    print("=" * 80)


def print_subheader(text):
    """Печать подзаголовка"""
    print("\n" + "-" * 60)
    print(f" {text}")
    print("-" * 60)


def inspect_module(module_name, depth=0):
    """Инспектирование модуля"""
    indent = "  " * depth
    try:
        module = importlib.import_module(module_name)
        print(f"{indent}✅ {module_name}")

        # Получаем все атрибуты модуля
        classes = []
        functions = []
        variables = []

        for name, obj in inspect.getmembers(module):
            if name.startswith('_'):
                continue
            if inspect.isclass(obj):
                classes.append(name)
            elif inspect.isfunction(obj):
                functions.append(name)
            elif not inspect.ismodule(obj):
                variables.append(name)

        if classes:
            print(f"{indent}  📦 Классы: {', '.join(classes[:5])}{'...' if len(classes) > 5 else ''}")
        if functions:
            print(f"{indent}  🔧 Функции: {', '.join(functions[:5])}{'...' if len(functions) > 5 else ''}")
        if variables:
            print(f"{indent}  📊 Переменные: {', '.join(variables[:5])}{'...' if len(variables) > 5 else ''}")

        return module
    except ImportError as e:
        print(f"{indent}❌ {module_name} - {e}")
        return None
    except Exception as e:
        print(f"{indent}⚠️ {module_name} - {e}")
        return None


def list_package_contents(package_name, prefix=""):
    """Рекурсивный обзор пакета"""
    try:
        package = importlib.import_module(package_name)
        package_path = os.path.dirname(package.__file__)

        print(f"\n📁 {package_name} ({package_path})")

        # Ищем все подмодули
        for _, name, is_pkg in pkgutil.iter_modules([package_path]):
            full_name = f"{package_name}.{name}"
            if is_pkg:
                list_package_contents(full_name, prefix + "  ")
            else:
                inspect_module(full_name, 1)

    except ImportError as e:
        print(f"❌ Cannot import {package_name}: {e}")
    except Exception as e:
        print(f"⚠️ Error inspecting {package_name}: {e}")


def check_file_exists(path):
    """Проверка существования файла"""
    if os.path.exists(path):
        print(f"✅ {path}")
        return True
    else:
        print(f"❌ {path}")
        return False


def main():
    print_header("AlKAsH3D Project Inspector")

    # Базовая директория проекта
    base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    alkash3d_dir = os.path.join(base_dir, 'alkash3d')

    print(f"Project root: {base_dir}")
    print(f"AlKAsH3D dir: {alkash3d_dir}")

    # Проверяем структуру директорий
    print_subheader("Directory Structure")

    dirs_to_check = [
        os.path.join(alkash3d_dir, 'core'),
        os.path.join(alkash3d_dir, 'graphics'),
        os.path.join(alkash3d_dir, 'graphics', 'renderers'),
        os.path.join(alkash3d_dir, 'graphics', 'backend'),
        os.path.join(alkash3d_dir, 'math'),
        os.path.join(alkash3d_dir, 'utils'),
        os.path.join(base_dir, 'resources'),
        os.path.join(base_dir, 'resources', 'shaders'),
    ]

    for d in dirs_to_check:
        check_file_exists(d)

    # Проверяем наличие __init__.py файлов
    print_subheader("Package Initialization Files")

    init_files = [
        os.path.join(alkash3d_dir, '__init__.py'),
        os.path.join(alkash3d_dir, 'core', '__init__.py'),
        os.path.join(alkash3d_dir, 'graphics', '__init__.py'),
        os.path.join(alkash3d_dir, 'graphics', 'renderers', '__init__.py'),
        os.path.join(alkash3d_dir, 'graphics', 'backend', '__init__.py'),
        os.path.join(alkash3d_dir, 'math', '__init__.py'),
        os.path.join(alkash3d_dir, 'utils', '__init__.py'),
    ]

    for init_file in init_files:
        check_file_exists(init_file)

    # Проверяем основные файлы
    print_subheader("Core Files")

    core_files = [
        os.path.join(alkash3d_dir, 'core', 'engine.py'),
        os.path.join(alkash3d_dir, 'core', 'config.py'),
        os.path.join(alkash3d_dir, 'graphics', 'renderers', 'forward_renderer.py'),
        os.path.join(alkash3d_dir, 'graphics', 'backend', 'dx12_backend.py'),
        os.path.join(alkash3d_dir, 'graphics', 'shader.py'),
        os.path.join(alkash3d_dir, 'graphics', 'mesh.py'),
        os.path.join(alkash3d_dir, 'graphics', 'material.py'),
        os.path.join(alkash3d_dir, 'graphics', 'texture.py'),
        os.path.join(alkash3d_dir, 'math', 'vector.py'),
        os.path.join(alkash3d_dir, 'math', 'matrix.py'),
    ]

    existing_files = []
    missing_files = []

    for core_file in core_files:
        if check_file_exists(core_file):
            existing_files.append(core_file)
        else:
            missing_files.append(core_file)

    # Инспектируем существующие модули
    if existing_files:
        print_subheader("Available Modules")

        # Добавляем путь к проекту в sys.path
        if base_dir not in sys.path:
            sys.path.insert(0, base_dir)

        # Пробуем импортировать модули
        modules_to_try = [
            'alkash3d.core.engine',
            'alkash3d.core.config',
            'alkash3d.graphics.renderers.forward_renderer',
            'alkash3d.graphics.backend.dx12_backend',
            'alkash3d.graphics.shader',
            'alkash3d.graphics.mesh',
            'alkash3d.graphics.material',
            'alkash3d.graphics.texture',
            'alkash3d.math.vector',
            'alkash3d.math.matrix',
        ]

        for module_name in modules_to_try:
            inspect_module(module_name)

    # Показываем содержимое alkash3d package
    print_subheader("Complete Package Structure")
    try:
        list_package_contents('alkash3d')
    except Exception as e:
        print(f"Error listing package: {e}")

    # Проверяем шейдеры
    print_subheader("Shader Files")

    shaders_dir = os.path.join(base_dir, 'resources', 'shaders')
    if os.path.exists(shaders_dir):
        shader_files = os.listdir(shaders_dir)
        for shader in shader_files:
            if shader.endswith('.hlsl'):
                print(f"✅ {shader}")
                # Показываем первые несколько строк шейдера
                try:
                    with open(os.path.join(shaders_dir, shader), 'r') as f:
                        lines = f.readlines()[:5]
                        for line in lines:
                            print(f"   {line.rstrip()}")
                except:
                    pass
    else:
        print("❌ Shaders directory not found")

    # Создаем недостающие файлы если нужно
    print_subheader("Missing Files Report")

    if missing_files:
        print("Следующие файлы отсутствуют:")
        for f in missing_files:
            print(f"  ❌ {f}")

        print("\nХотите создать недостающие файлы? (y/n)")
        # response = input().lower()
        # if response == 'y':
        #     create_missing_files(missing_files)
    else:
        print("✅ Все основные файлы присутствуют!")


def create_missing_files(files):
    """Создание недостающих файлов"""
    for file_path in files:
        dir_path = os.path.dirname(file_path)
        if not os.path.exists(dir_path):
            os.makedirs(dir_path)

        if not os.path.exists(file_path):
            with open(file_path, 'w') as f:
                f.write(f'# {os.path.basename(file_path)}\n')
                f.write('"""Auto-generated file"""\n\n')

                # Добавляем базовый класс для некоторых файлов
                if 'vector.py' in file_path:
                    f.write('class Vector3:\n')
                    f.write('    def __init__(self, x=0, y=0, z=0):\n')
                    f.write('        self.x = x\n')
                    f.write('        self.y = y\n')
                    f.write('        self.z = z\n\n')
                elif 'matrix.py' in file_path:
                    f.write('class Matrix4x4:\n')
                    f.write('    @staticmethod\n')
                    f.write('    def identity():\n')
                    f.write('        return Matrix4x4()\n\n')
                    f.write('    @staticmethod\n')
                    f.write('    def look_at(eye, target, up):\n')
                    f.write('        return Matrix4x4()\n\n')
                    f.write('    @staticmethod\n')
                    f.write('    def perspective(fov, aspect, near, far):\n')
                    f.write('        return Matrix4x4()\n')

            print(f"Created: {file_path}")


if __name__ == "__main__":
    main()