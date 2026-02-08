# alkash3d/multithread/__init__.py
"""
Пакет, содержащий простой пул задач на основе concurrent.futures.

Сейчас в пакете есть только один класс – TaskPool,
который экспортируется здесь для удобного импорта:

    from alkash3d.multithread import TaskPool
"""

from alkash3d.multithread.task_pool import TaskPool

__all__ = ["TaskPool"]
