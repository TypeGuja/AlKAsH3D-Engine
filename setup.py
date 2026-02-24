#!/usr/bin/env python3
# -*- coding: utf-8 -*-

from setuptools import setup, find_packages
import os
import sys
import platform
from setuptools.command.install import install
from setuptools.command.develop import develop
from setuptools.command.egg_info import egg_info
import subprocess
import shutil

# Определяем текущую платформу
IS_WINDOWS = platform.system() == "Windows"
IS_LINUX = platform.system() == "Linux"
IS_MAC = platform.system() == "Darwin"

# Пути к библиотекам
LIB_EXT = ".pyd" if IS_WINDOWS else ".so" if IS_LINUX else ".dylib"
LIB_NAME = f"AlKAsH3D{LIB_EXT}"


class BuildAlKAsH3D:
    """Класс для сборки движка AlKAsH3D"""

    @staticmethod
    def build_engine():
        """Сборка движка через CMake"""
        print("=" * 60)
        print("Сборка AlKAsH3D Engine...")
        print("=" * 60)

        # Проверяем наличие CMake
        try:
            subprocess.run(["cmake", "--version"], check=True, capture_output=True)
        except (subprocess.CalledProcessError, FileNotFoundError):
            print("ОШИБКА: CMake не найден. Установите CMake:")
            if IS_WINDOWS:
                print("  https://cmake.org/download/")
            elif IS_LINUX:
                print("  sudo apt install cmake  # или sudo dnf install cmake")
            elif IS_MAC:
                print("  brew install cmake")
            sys.exit(1)

        # Создаём папку для сборки
        build_dir = os.path.join(os.path.dirname(__file__), "build_temp")
        if not os.path.exists(build_dir):
            os.makedirs(build_dir)

        # Конфигурируем CMake
        cmake_args = [
            "cmake",
            "..",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DBUILD_PYTHON_BINDINGS=ON",
            "-DBUILD_EXAMPLES=OFF",
            "-DBUILD_TESTS=OFF"
        ]

        # Добавляем специфичные для платформы аргументы
        if IS_WINDOWS:
            cmake_args.append("-A x64")

        try:
            subprocess.run(cmake_args, cwd=build_dir, check=True)
        except subprocess.CalledProcessError as e:
            print(f"ОШИБКА при конфигурации CMake: {e}")
            sys.exit(1)

        # Собираем проект
        try:
            subprocess.run(["cmake", "--build", ".", "--config", "Release", "-j4"],
                           cwd=build_dir, check=True)
        except subprocess.CalledProcessError as e:
            print(f"ОШИБКА при сборке: {e}")
            sys.exit(1)

        # Копируем библиотеку в нужное место
        lib_source = os.path.join(build_dir, "python", LIB_NAME)
        lib_dest = os.path.join(os.path.dirname(__file__), "alkash3d", LIB_NAME)

        if os.path.exists(lib_source):
            shutil.copy2(lib_source, lib_dest)
            print(f"Библиотека скопирована в {lib_dest}")
        else:
            print(f"ПРЕДУПРЕЖДЕНИЕ: Библиотека не найдена по пути {lib_source}")
            # Ищем в других местах
            possible_paths = [
                os.path.join(build_dir, LIB_NAME),
                os.path.join(build_dir, "Release", LIB_NAME),
                os.path.join(build_dir, "python", "Release", LIB_NAME),
            ]
            for path in possible_paths:
                if os.path.exists(path):
                    shutil.copy2(path, lib_dest)
                    print(f"Библиотека найдена и скопирована из {path}")
                    break

        print("=" * 60)
        print("Сборка завершена успешно!")
        print("=" * 60)


# Кастомные команды установки
class InstallCommand(install):
    def run(self):
        BuildAlKAsH3D.build_engine()
        super().run()


class DevelopCommand(develop):
    def run(self):
        BuildAlKAsH3D.build_engine()
        super().run()


class EggInfoCommand(egg_info):
    def run(self):
        # Не собираем движок для egg_info
        super().run()


# Читаем README для long_description
with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

# Настройка пакета
setup(
    name="alkash3d-python",
    version="0.1.0",
    author="TypeGuja",
    author_email="ваш_email@example.com",
    description="Python биндинги для AlKAsH3D Engine",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/TypeGuja/AlKAsH3D-Engine",

    packages=find_packages(),
    package_data={
        "alkash3d": [f"*{LIB_EXT}", "*.dll", "*.so", "*.dylib"],
    },
    include_package_data=True,

    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "Topic :: Multimedia :: Graphics",
        "Topic :: Games/Entertainment",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.6",
        "Programming Language :: Python :: 3.7",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Operating System :: OS Independent",
    ],

    python_requires=">=3.6",
    install_requires=[
        "numpy>=1.19.0",
        "pillow>=8.0.0",  # Для загрузки текстур
    ],

    extras_require={
        "dev": [
            "pytest>=6.0",
            "pytest-cov",
            "black",
            "flake8",
        ],
        "examples": [
            "pygame",  # Для примеров
        ],
    },

    cmdclass={
        "install": InstallCommand,
        "develop": DevelopCommand,
        "egg_info": EggInfoCommand,
    },

    entry_points={
        "console_scripts": [
            "alkash3d-run=alkash3d.runner:main",
        ],
    },

    zip_safe=False,
)