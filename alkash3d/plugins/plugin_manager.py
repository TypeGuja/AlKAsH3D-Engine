"""
Простейший менеджер плагинов.
Плагины – обычные модули, содержащие функцию `register(manager)`.
"""

import importlib
import os
from pathlib import Path
from typing import Dict, Any, Optional


class PluginManager:
    """Сканирует подпапку `plugins/` и регистрирует найденные passes."""

    def __init__(self, plugins_dir: Optional[Path] = None):
        if plugins_dir is None:
            plugins_dir = Path(__file__).parent
        self.dir = plugins_dir
        self.passes: Dict[str, Any] = {}

    def discover(self) -> None:
        """Импортировать все модули и вызвать `register`."""
        # ИСПРАВЛЕНИЕ: используем os.listdir вместо pkgutil.iter_modules
        if not os.path.exists(self.dir):
            print(f"[PluginManager] Directory not found: {self.dir}")
            return

        for file in os.listdir(self.dir):
            if file.endswith('.py') and not file.startswith('__'):
                module_name = file[:-3]  # убираем .py
                try:
                    module = importlib.import_module(f"alkash3d.plugins.{module_name}")
                    if hasattr(module, "register"):
                        module.register(self)
                        print(f"[PluginManager] Loaded plugin: {module_name}")
                except Exception as e:
                    print(f"[PluginManager] Failed to load {module_name}: {e}")

    def register_pass(self, name: str, pass_cls: Any) -> None:
        """Зарегистрировать проход рендеринга."""
        self.passes[name] = pass_cls

    def get_pass(self, name: str) -> Optional[Any]:
        """Получить проход по имени."""
        return self.passes.get(name)