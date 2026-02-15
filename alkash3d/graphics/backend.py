"""
Абстрактный интерфейс для графических бекендов.
"""

from __future__ import annotations
from abc import ABC, abstractmethod
from typing import Any, Sequence, Tuple, Optional

class GraphicsBackend(ABC):
    """Base interface for graphics backends."""

    @abstractmethod
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        pass

    @abstractmethod
    def resize(self, width: int, height: int) -> None:
        pass

    @abstractmethod
    def present(self) -> None:
        pass

    @abstractmethod
    def compile_shader(self, stage: str, source_path: str) -> Any:
        pass

    @abstractmethod
    def create_graphics_ps(self, vs_blob: Any, ps_blob: Any) -> Any:
        pass

    @abstractmethod
    def set_graphics_pipeline(self, pso: Any) -> None:
        pass

    @abstractmethod
    def create_buffer(self, data: bytes, usage: str = "default") -> Any:
        pass

    @abstractmethod
    def update_buffer(self, buffer: Any, data: bytes) -> None:
        pass

    @abstractmethod
    def create_texture(
        self,
        data: Optional[bytes],
        width: int,
        height: int,
        fmt: str = "RGBA8"
    ) -> Any:
        pass

    @abstractmethod
    def create_constant_buffer(self, data: bytes) -> Any:
        pass

    @abstractmethod
    def update_texture(
        self,
        texture: Any,
        data: bytes,
        width: int,
        height: int
    ) -> None:
        pass

    @abstractmethod
    def create_descriptor_heap(
        self,
        num_descriptors: int,
        heap_type: str = "cbv_srv_uav"
    ) -> Any:
        pass

    @abstractmethod
    def get_cpu_handle(self, heap: Any, index: int) -> int:
        pass

    @abstractmethod
    def get_gpu_handle(self, heap: Any, index: int) -> int:
        pass

    @abstractmethod
    def set_root_descriptor_table(self, root_index: int, gpu_handle: int) -> None:
        pass

    @abstractmethod
    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> None:
        pass

    @abstractmethod
    def set_render_target(self, rtv: int) -> None:
        pass

    @abstractmethod
    def set_render_targets(self, rtvs: Sequence[int]) -> None:
        pass

    @abstractmethod
    def clear_render_target(
        self,
        rtv: int,
        color: Tuple[float, float, float, float]
    ) -> None:
        pass

    @abstractmethod
    def set_viewport(
        self,
        x: int, y: int, width: int, height: int,
        min_depth: float = 0.0, max_depth: float = 1.0
    ) -> None:
        pass

    @abstractmethod
    def set_scissor_rect(
        self,
        left: int, top: int,
        right: int, bottom: int
    ) -> None:
        pass

    @abstractmethod
    def set_vertex_buffers(
        self,
        vertex_buffer: Any,
        index_buffer: Optional[Any] = None
    ) -> None:
        pass

    @abstractmethod
    def draw(
        self,
        vertex_count: int,
        start_vertex: int = 0,
        instance_count: int = 1
    ) -> None:
        pass

    @abstractmethod
    def draw_indexed(
        self,
        index_count: int,
        start_index: int = 0,
        base_vertex: int = 0,
        instance_count: int = 1
    ) -> None:
        pass

    @abstractmethod
    def draw_fullscreen_quad(
        self,
        pso: Any,
        descriptor_heaps: Sequence[Any],
        root_parameters: Sequence[Tuple[int, int]]
    ) -> None:
        pass

    @abstractmethod
    def wait_for_gpu(self) -> None:
        pass

    @abstractmethod
    def release_resource(self, resource: Any) -> None:
        pass

    @abstractmethod
    def enable_depth_test(self, enable: bool) -> None:
        pass

    @abstractmethod
    def begin_frame(self) -> None:
        pass

    @abstractmethod
    def end_frame(self) -> None:
        pass

    @abstractmethod
    def shutdown(self) -> None:
        pass

def select_backend(name: str = "dx12") -> GraphicsBackend:
    """Select graphics backend by name."""
    name = name.lower()
    if name == "dx12":
        from .dx12_backend import DX12Backend
        return DX12Backend()
    elif name == "gl":
        from .gl_backend import GLBackend
        return GLBackend()
    else:
        raise ValueError(f"Unknown graphics backend: {name}")