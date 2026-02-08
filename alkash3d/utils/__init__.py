# alkash3d/utils/__init__.py
"""
Пакет утилит.

Экспортируем:
    * logger      – готовый объект logging.Logger (с level INFO)
    * gl_check_error – вспомогательная функция, проверяющая GL‑ошибки
"""

# Импортируем объект logger и функцию из модуля logger.py
from .logger import logger, gl_check_error

# Чтобы `from alkash3d.utils import *` не экспортировал лишнее
__all__ = ["logger", "gl_check_error"]
