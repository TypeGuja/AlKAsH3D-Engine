# alkash3d/renderer/__init__.py
"""
Экспорт основных рендер‑компонентов.
"""
from alkash3d.renderer.base_renderer import BaseRenderer
from alkash3d.renderer.shader import Shader
from alkash3d.renderer.pipelines.forward import ForwardRenderer
from alkash3d.renderer.pipelines.deferred import DeferredRenderer
from alkash3d.renderer.pipelines.rt_pipeline import RTPipeline

__all__ = ["BaseRenderer", "Shader", "ForwardRenderer", "DeferredRenderer", "RTPipeline"]
