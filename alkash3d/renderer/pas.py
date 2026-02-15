"""
Базовый интерфейс для одиночного рендера‑прохода (post‑process).
"""

from abc import ABC, abstractmethod

class RenderPass(ABC):
    """Один проход в цепочке пост‑процессинга."""
    @abstractmethod
    def init(self, width: int, height: int, backend) -> None:
        pass

    @abstractmethod
    def run(self, src_tex: int, backend) -> int:
        pass

    @abstractmethod
    def resize(self, w: int, h: int, backend) -> None:
        pass

    @abstractmethod
    def cleanup(self, backend) -> None:
        pass
