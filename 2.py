#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Скрипт для поиска всех [API] логов в проекте AlKAsH3D Engine.
"""

import os
import re
import sys
from pathlib import Path
from typing import List, Tuple, Dict, Set


class APILogFinder:
    """Поиск [API] логов в файлах проекта."""

    def __init__(self, root_path: str = None):
        """Инициализация поисковика."""
        if root_path is None:
            # Определяем корень проекта
            self.root_path = Path(__file__).parent
        else:
            self.root_path = Path(root_path)

        self.results: List[Tuple[Path, int, str]] = []
        self.api_macros: Set[str] = set()

    def find_rust_files(self) -> List[Path]:
        """Находит все Rust файлы в проекте."""
        rust_files = []

        # Паттерны для поиска
        patterns = [
            "*.rs",
            "**/*.rs",
        ]

        for pattern in patterns:
            rust_files.extend(self.root_path.glob(pattern))

        return rust_files

    def find_python_files(self) -> List[Path]:
        """Находит все Python файлы в проекте."""
        python_files = []

        # Паттерны для поиска
        patterns = [
            "*.py",
            "**/*.py",
        ]

        # Исключаем виртуальное окружение
        for pattern in patterns:
            for file in self.root_path.glob(pattern):
                if ".venv" not in str(file) and "site-packages" not in str(file):
                    python_files.append(file)

        return python_files

    def search_in_rust_file(self, file_path: Path) -> List[Tuple[Path, int, str]]:
        """Ищет [API] логи в Rust файле."""
        results = []

        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                lines = f.readlines()

            for i, line in enumerate(lines, 1):
                # Ищем [API] в логах
                if '[API]' in line or 'debug_println!("[API]' in line:
                    # Очищаем строку от лишних пробелов
                    clean_line = line.strip()
                    results.append((file_path, i, clean_line))

                    # Собираем макросы
                    if 'debug_println!("[API]' in line:
                        self.api_macros.add('debug_println')
                    if 'eprintln!("[API]' in line:
                        self.api_macros.add('eprintln')
                    if 'println!("[API]' in line:
                        self.api_macros.add('println')

        except Exception as e:
            print(f"Error reading {file_path}: {e}")

        return results

    def search_in_python_file(self, file_path: Path) -> List[Tuple[Path, int, str]]:
        """Ищет [API] логи в Python файле."""
        results = []

        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                lines = f.readlines()

            for i, line in enumerate(lines, 1):
                # Ищем [API] в логах Python
                if '[API]' in line:
                    clean_line = line.strip()
                    results.append((file_path, i, clean_line))

        except Exception as e:
            print(f"Error reading {file_path}: {e}")

        return results

    def search_all(self) -> Dict[str, List[Tuple[Path, int, str]]]:
        """Поиск во всех файлах."""
        results = {
            'rust': [],
            'python': []
        }

        print(f"Searching in: {self.root_path}")
        print("-" * 60)

        # Поиск в Rust файлах
        rust_files = self.find_rust_files()
        print(f"Found {len(rust_files)} Rust files")

        for file in rust_files:
            file_results = self.search_in_rust_file(file)
            if file_results:
                results['rust'].extend(file_results)
                print(f"  Found in: {file.relative_to(self.root_path)} - {len(file_results)} matches")

        # Поиск в Python файлах
        python_files = self.find_python_files()
        print(f"Found {len(python_files)} Python files")

        for file in python_files:
            file_results = self.search_in_python_file(file)
            if file_results:
                results['python'].extend(file_results)
                print(f"  Found in: {file.relative_to(self.root_path)} - {len(file_results)} matches")

        return results

    def print_results(self, results: Dict[str, List[Tuple[Path, int, str]]]):
        """Выводит результаты поиска."""
        print("\n" + "=" * 80)
        print("RESULTS")
        print("=" * 80)

        # Rust файлы
        if results['rust']:
            print(f"\n📝 Rust files ({len(results['rust'])} matches):")
            print("-" * 60)
            for file, line_num, content in results['rust']:
                rel_path = file.relative_to(self.root_path)
                print(f"  📄 {rel_path}:{line_num}")
                print(f"     {content[:100]}")
                print()

        # Python файлы
        if results['python']:
            print(f"\n🐍 Python files ({len(results['python'])} matches):")
            print("-" * 60)
            for file, line_num, content in results['python']:
                rel_path = file.relative_to(self.root_path)
                print(f"  📄 {rel_path}:{line_num}")
                print(f"     {content[:100]}")
                print()

        # Статистика
        print("\n" + "=" * 80)
        print("STATISTICS")
        print("=" * 80)
        print(f"Total matches: {len(results['rust']) + len(results['python'])}")
        print(f"  - Rust files: {len(results['rust'])}")
        print(f"  - Python files: {len(results['python'])}")

        if self.api_macros:
            print(f"\nAPI logging macros used:")
            for macro in sorted(self.api_macros):
                print(f"  - {macro}")

    def export_to_file(self, results: Dict[str, List[Tuple[Path, int, str]]], output_file: str = "api_logs_report.txt"):
        """Экспортирует результаты в файл."""
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write("=" * 80 + "\n")
            f.write("API LOGS REPORT\n")
            f.write("=" * 80 + "\n\n")
            f.write(f"Project root: {self.root_path}\n")
            f.write(f"Total matches: {len(results['rust']) + len(results['python'])}\n\n")

            # Rust файлы
            if results['rust']:
                f.write("\n📝 RUST FILES:\n")
                f.write("-" * 60 + "\n")
                for file, line_num, content in results['rust']:
                    rel_path = file.relative_to(self.root_path)
                    f.write(f"\n📄 {rel_path}:{line_num}\n")
                    f.write(f"   {content}\n")

            # Python файлы
            if results['python']:
                f.write("\n🐍 PYTHON FILES:\n")
                f.write("-" * 60 + "\n")
                for file, line_num, content in results['python']:
                    rel_path = file.relative_to(self.root_path)
                    f.write(f"\n📄 {rel_path}:{line_num}\n")
                    f.write(f"   {content}\n")

        print(f"\n📄 Report exported to: {output_file}")

    def find_api_calls(self) -> List[Tuple[Path, int, str]]:
        """Находит вызовы API функций в Rust коде."""
        api_calls = []

        rust_files = self.find_rust_files()
        for file in rust_files:
            try:
                with open(file, 'r', encoding='utf-8') as f:
                    lines = f.readlines()

                for i, line in enumerate(lines, 1):
                    # Ищем экспортируемые функции
                    if '#[no_mangle]' in line:
                        # Смотрим следующую строку для имени функции
                        if i < len(lines):
                            next_line = lines[i].strip()
                            if 'pub extern "C" fn' in next_line:
                                # Извлекаем имя функции
                                match = re.search(r'fn (\w+)', next_line)
                                if match:
                                    func_name = match.group(1)
                                    api_calls.append((file, i, f"API function: {func_name}"))
            except Exception as e:
                print(f"Error reading {file}: {e}")

        return api_calls


def main():
    """Главная функция."""
    # Определяем корень проекта
    script_path = Path(__file__).resolve()

    # Ищем корень проекта (где находится alkash3d)
    root = script_path.parent
    while root.parent != root:
        if (root / "alkash3d").exists():
            break
        root = root.parent

    print(f"Project root: {root}")
    print()

    # Создаём поисковик
    finder = APILogFinder(root)

    # Поиск всех [API] логов
    results = finder.search_all()

    # Вывод результатов
    finder.print_results(results)

    # Поиск API функций
    print("\n" + "=" * 80)
    print("API FUNCTIONS")
    print("=" * 80)

    api_functions = finder.find_api_calls()
    if api_functions:
        for file, line_num, func in api_functions:
            rel_path = file.relative_to(root)
            print(f"  📄 {rel_path}:{line_num} - {func}")
    else:
        print("  No API functions found")

    # Экспорт в файл
    finder.export_to_file(results, "api_logs_report.txt")

    print("\n✅ Search completed!")


if __name__ == "__main__":
    main()