# alkash3d/postproc/ssao.py
from alkash3d.renderer.pas import RenderPass

class SSAOPass(RenderPass):
    def __init__(self, radius=0.5, bias=0.025):
        self.radius = radius
        self.bias = bias

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
