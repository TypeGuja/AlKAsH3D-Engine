"""
Контекст‑менеджер профайлинга – измеряет время выполнения блока кода.
"""

import time
from alkash3d.utils.logger import logger

class Profiler:
    """Контекст‑менеджер для измерения времени выполнения."""
    def __init__(self, name: str):
        self.name = name
        self._start = 0.0

    def __enter__(self):
        self._start = time.perf_counter()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        elapsed = (time.perf_counter() - self._start) * 1000.0  # ms
        logger.debug(f"[Profiler] {self.name}: {elapsed:.2f} ms")
