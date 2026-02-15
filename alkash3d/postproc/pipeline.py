# alkash3d/postproc/pipeline.py
"""
Контейнер для цепочки пост‑процессов.
Каждый Pass реализует интерфейс `RenderPass` (из renderer.pas).
"""

from alkash3d.renderer.pas import RenderPass
from typing import Optional, Any


class PostProcessingPipeline:
    """Контейнер для набора RenderPass‑ов."""

    def __init__(self, width: int, height: int):
        self.width = width
        self.height = height
        self.passes: list[RenderPass] = []
        self.backend = None  # будет установлен в Engine при создании

    # -----------------------------------------------------------------
    def add_pass(self, rp: RenderPass) -> None:
        """Регистрация нового прохода (создаёт ресурсы)."""
        if self.backend is None:
            raise RuntimeError("PostProcessingPipeline: backend not set")
        rp.init(self.width, self.height, self.backend)
        self.passes.append(rp)

    # -----------------------------------------------------------------
    def run(self, backend) -> Optional[Any]:
        """
        Выполняет цепочку пассов, начиная с ``src_texture``.
        Если ``src_texture`` is None (например, forward‑renderer),
        просто пропускаем.
        """
        if not self.passes:
            return None

        cur = None
        for rp in self.passes:
            cur = rp.run(cur, backend)
            if cur is None:
                break
        return cur

    # -----------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        self.width, self.height = w, h
        for rp in self.passes:
            rp.resize(w, h, self.backend)

    # -----------------------------------------------------------------
    def cleanup(self, backend) -> None:
        for rp in self.passes:
            rp.cleanup(backend)