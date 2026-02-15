"""
Простейший менеджер плагинов.
Плагины – обычные модули, содержащие функцию `register(manager)`.
"""

import importlib
import pkgutil
from pathlib import Path

class PluginManager:
    """Сканирует подпапку `plugins/` и регистрирует найденные passes."""
    def __init__(self, plugins_dir: Path = None):
        if plugins_dir is None:
            plugins_dir = Path(__file__).parent
        self.dir = plugins_dir
        self.passes = {}

    def discover(self):
        """Импортировать все модули и вызвать `register`."""
        for modinfo in pkgutil.iter_modules([str(self.dir)]):
            module = importlib.import_module(f"alkash3d.plugins.{modinfo.name}")
            if hasattr(module, "register"):
                module.register(self)

    def register_pass(self, name: str, pass_cls):
        self.passes[name] = pass_cls

    def get_pass(self, name: str):
        return self.passes.get(name)
