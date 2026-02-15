"""
OpenGL backend stub.
"""

from typing import Any, Sequence, Tuple, Optional

from alkash3d.graphics.backend import GraphicsBackend

class GLBackend(GraphicsBackend):
    """OpenGL backend (not implemented)."""

    def __init__(self):
        raise NotImplementedError("OpenGL backend is not implemented")

    # All abstract methods raise NotImplementedError – left as a stub.
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        raise NotImplementedError()

    def resize(self, width: int, height: int) -> None:
        raise NotImplementedError()

    def present(self) -> None:
        raise NotImplementedError()

    def compile_shader(self, stage: str, source_path: str) -> Any:
        raise NotImplementedError()

    def create_graphics_ps(self, vs_blob: Any, ps_blob: Any) -> Any:
        raise NotImplementedError()

    def set_graphics_pipeline(self, pso: Any) -> None:
        raise NotImplementedError()

    def create_buffer(self, data: bytes, usage: str = "default") -> Any:
        raise NotImplementedError()

    def update_buffer(self, buffer: Any, data: bytes) -> None:
        raise NotImplementedError()

    def create_texture(self,
            data: Optional[bytes],
            width: int,
            height: int,
            fmt: str = "RGBA8"
    ) -> Any:
        raise NotImplementedError()

    def create_constant_buffer(self, data: bytes) -> Any:
        raise NotImplementedError()

    def update_texture(self,
            texture: Any,
            data: bytes,
            width: int,
            height: int
    ) -> None:
        raise NotImplementedError()

    def create_descriptor_heap(self,
            num_descriptors: int,
            heap_type: str = "cbv_srv_uav"
    ) -> Any:
        raise NotImplementedError()

    def get_cpu_handle(self, heap: Any, index: int) -> int:
        raise NotImplementedError()

    def get_gpu_handle(self, heap: Any, index: int) -> int:
        raise NotImplementedError()

    def set_root_descriptor_table(self, root_index: int, gpu_handle: int) -> None:
        raise NotImplementedError()

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> None:
        raise NotImplementedError()

    def set_render_target(self, rtv: int) -> None:
        raise NotImplementedError()

    def set_render_targets(self, rtvs: Sequence[int]) -> None:
        raise NotImplementedError()

    def clear_render_target(self,
            rtv: int,
            color: Tuple[float, float, float, float]
    ) -> None:
        raise NotImplementedError()

    def set_viewport(self,
            x: int, y: int, width: int, height: int,
            min_depth: float = 0.0, max_depth: float = 1.0
    ) -> None:
        raise NotImplementedError()

    def set_scissor_rect(self,
            left: int, top: int,
            right: int, bottom: int
    ) -> None:
        raise NotImplementedError()

    def set_vertex_buffers(self,
            vertex_buffer: Any,
            index_buffer: Optional[Any] = None
    ) -> None:
        raise NotImplementedError()

    def draw(self,
            vertex_count: int,
            start_vertex: int = 0,
            instance_count: int = 1
    ) -> None:
        raise NotImplementedError()

    def draw_indexed(self,
            index_count: int,
            start_index: int = 0,
            base_vertex: int = 0,
            instance_count: int = 1
    ) -> None:
        raise NotImplementedError()

    def draw_fullscreen_quad(self,
            pso: Any,
            descriptor_heaps: Sequence[Any],
            root_parameters: Sequence[Tuple[int, int]]
    ) -> None:
        raise NotImplementedError()

    def wait_for_gpu(self) -> None:
        raise NotImplementedError()

    def release_resource(self, resource: Any) -> None:
        raise NotImplementedError()

    def enable_depth_test(self, enable: bool) -> None:
        raise NotImplementedError()

    def begin_frame(self) -> None:
        raise NotImplementedError()

    def end_frame(self) -> None:
        raise NotImplementedError()

    def shutdown(self) -> None:
        raise NotImplementedError()