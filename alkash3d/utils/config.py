"""
Простой загрузчик/сохранитель конфигурации в формате JSON.
Если файл не найден – создаётся файл с настройками по‑умолчанию.
"""

import json
from pathlib import Path
from alkash3d.utils.logger import logger

DEFAULT_CONFIG = {
    "window": {"width": 1280, "height": 720, "title": "AlKAsH3D Engine"},
    "v_sync": True,
    "show_fps": True,
    "upscale": {"enabled": False, "mode": "fsr", "quality": "medium"},
    "editor_app": False,
}

class Config:
    """Singleton‑подобный объект конфигурации."""
    _instance = None

    def __new__(cls, path: str = "config.json"):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance.path = Path(path)
            cls._instance._load()
        return cls._instance

    def _load(self):
        if self.path.is_file():
            try:
                with self.path.open("r", encoding="utf-8") as f:
                    self.data = json.load(f)
                logger.info("[Config] Loaded configuration.")
            except Exception as exc:
                logger.error(f"[Config] Failed to read config: {exc}")
                self.data = DEFAULT_CONFIG.copy()
                self.save()
        else:
            logger.info("[Config] No config file – creating default.")
            self.data = DEFAULT_CONFIG.copy()
            self.save()

    def save(self):
        try:
            with self.path.open("w", encoding="utf-8") as f:
                json.dump(self.data, f, indent=4)
            logger.info("[Config] Configuration saved.")
        except Exception as exc:
            logger.error(f"[Config] Unable to save config: {exc}")

    def __getitem__(self, key):
        return self.data.get(key, DEFAULT_CONFIG.get(key))

    def __setitem__(self, key, value):
        self.data[key] = value
        self.save()

    def get(self, key, default=None):
        return self.data.get(key, default)