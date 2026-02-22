"""
Таймер с высоким разрешением (nanosecond precision).
"""

import time

class Timer:
    """Таймер с высоким разрешением (nanosecond precision)."""
    def __init__(self):
        self._last = time.perf_counter()
        self.delta = 0.0
        self.fps = 0.0

    def tick(self) -> float:
        """Обновить таймер, вернуть dt в секундах."""
        now = time.perf_counter()
        self.delta = now - self._last
        self._last = now
        self.fps = 1.0 / self.delta if self.delta > 0.0 else 0.0
        return self.delta
