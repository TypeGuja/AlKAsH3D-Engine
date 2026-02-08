# alkas3d/renderer/pipelines/rt_pipeline.py
# ---------------------------------------------------------------
# Wrapper‑класс, чтобы можно было выбрать «rt» в Engine и
# автоматически использовать RayTracer (CUDA‑ядро) в качестве рендерера.
# ---------------------------------------------------------------
from alkash3d.renderer.raytracer import RayTracer
from alkash3d.renderer.base_renderer import BaseRenderer


class RTPipeline(BaseRenderer):
    def __init__(self, window):
        self.tracer = RayTracer(window)

    def resize(self, w, h):
        self.tracer.resize(w, h)

    def render(self, scene, camera):
        # На данный момент наш RayTracer не использует сцену,
        # но в реальном проекте сюда передадим массив объектов, их
        # материал и трассировочный ускоритель (BVH).
        self.tracer.render(scene, camera)
