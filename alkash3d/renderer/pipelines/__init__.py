# alkash3d/renderer/pipelines/__init__.py
"""
Пакет с готовыми пайплайнами.
"""

from alkash3d.renderer.pipelines.forward import ForwardRenderer
from alkash3d.renderer.pipelines.deferred import DeferredRenderer
from alkash3d.renderer.pipelines.rt_pipeline import RTPipeline

__all__ = ["ForwardRenderer", "DeferredRenderer", "RTPipeline"]
