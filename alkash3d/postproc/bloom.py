# alkash3d/postproc/bloom.py
from alkash3d.renderer.pas import RenderPass

class BloomPass(RenderPass):
    """Плейс‑холдер – ничего не делает, лишь передаёт текстуру дальше."""
    def __init__(self, threshold=1.0, intensity=1.2):
        self.threshold = threshold
        self.intensity = intensity

    def init(self, width, height, backend):
        self.backend = backend
        self.width, self.height = width, height

    def run(self, src_tex, backend):
        # просто пропускаем входную текстуру
        return src_tex

    def resize(self, w, h, backend):
        self.width, self.height = w, h
        self.backend = backend

    def cleanup(self, backend):
        pass