# alkas3d/core/timer.py
# ---------------------------------------------------------------
# Минимальный таймер (для измерения FPS и профайлинга).
# ---------------------------------------------------------------
import time


class Timer:
    """Простой FPS‑таймер."""
    def __init__(self):
        self._last = time.time()
        self.delta = 0.0
        self.fps = 0.0

    def tick(self):
        now = time.time()
        self.delta = now - self._last
        self._last = now
        self.fps = 1.0 / self.delta if self.delta > 0 else 0.0
        return self.delta
