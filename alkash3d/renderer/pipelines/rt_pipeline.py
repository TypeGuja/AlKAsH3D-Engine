# alkash3d/renderer/pipelines/rt_pipeline.py
"""
Обёртка‑класс, позволяющая выбрать «rt» в Engine и автоматически
использовать RayTracer (CUDA‑ядро) в качестве рендерера.
"""

from alkash3d.renderer.raytracer import RayTracer
from alkash3d.renderer.base_renderer import BaseRenderer


class RTPipeline(BaseRenderer):
    def __init__(self, window):
        self.tracer = RayTracer(window)

    def resize(self, w, h):
        self.tracer.resize(w, h)

    def render(self, scene, camera):
        self.tracer.render(scene, camera)