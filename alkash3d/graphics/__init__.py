"""
Графический слой – выбирает нужный бэкенд (OpenGL или DirectX 12).
"""

from alkash3d.graphics.backend import GraphicsBackend, select_backend
from alkash3d.graphics.gl_backend import GLBackend
from alkash3d.graphics.dx12_backend import DX12Backend

__all__ = [
    "GraphicsBackend",
    "GLBackend",
    "DX12Backend",
    "select_backend",
]
