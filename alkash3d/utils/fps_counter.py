"""
Простой FPS‑счётчик. Можно использовать в любом месте,
например, в Engine.run() или в пользовательском UI.
"""

import time
from collections import deque

class FPSCounter:
    """Скользящее среднее FPS за последние N измерений."""
    def __init__(self, window_size: int = 30):
        self._times = deque(maxlen=window_size)
        self.last = time.time()
        self.fps = 0.0

    def tick(self) -> float:
        """Обновить счётчик, вернуть дельту в секундах."""
        now = time.time()
        dt = now - self.last
        self.last = now
        self._times.append(dt)
        if len(self._times) == self._times.maxlen:
            avg = sum(self._times) / len(self._times)
            self.fps = 1.0 / avg if avg > 0 else 0.0
        else:
            self.fps = 1.0 / dt if dt > 0 else 0.0
        return dt