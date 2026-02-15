# alkash3d/postproc/color_grading.py
from alkash3d.renderer.pas import RenderPass

class ColorGradingPass(RenderPass):
    def __init__(self):
        self.lut_path = None

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