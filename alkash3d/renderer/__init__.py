"""
Экспорт основных рендер‑компонентов, а также фабрики.
"""

from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader
from alkash3d.renderer.pipelines.forward import ForwardRenderer
from alkash3d.renderer.pipelines.deferred import DeferredRenderer
from alkash3d.renderer.pipelines.hybrid import HybridRenderer
from alkash3d.renderer.pipelines.rtx_renderer import RTXRenderer
from alkash3d.renderer.pas import RenderPass

__all__ = [
    "BaseRenderer",
    "Shader",
    "ForwardRenderer",
    "DeferredRenderer",
    "HybridRenderer",
    "RTXRenderer",
    "RenderPass",
]
