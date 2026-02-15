# editor_app/__init__.py
"""
Отдельный Qt‑редактор для AlKAsH3D.

Экспортирует единственную функцию ``run`` – точку входа,
чтобы её можно было вызвать так:

    from editor_app import run
    run()

Или из командной строки:

    python -m editor_app
"""

from .main import run

__all__ = ["run"]
