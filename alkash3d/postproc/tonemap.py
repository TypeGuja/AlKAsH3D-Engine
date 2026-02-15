# alkash3d/postproc/tonemap.py
from alkash3d.renderer.pas import RenderPass

class TonemapPass(RenderPass):
    def __init__(self):
        pass

    def init(self, width, height, backend):
        self.backend = backend
        self.width, self.height = width, height

    def run(self, src_tex, backend):
        return src_tex

    def resize(self, w, h, backend):
        self.width, self.height = w, h
        self.backend = backend

    def cleanup(self, backend):
        pass