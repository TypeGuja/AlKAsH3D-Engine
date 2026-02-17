# -*- coding: utf-8 -*-
"""
Общие фикстуры и «мок‑бэкенд», который записывает каждый вызов в список.
"""

import pytest
import ctypes
from typing import Any, Sequence, Tuple

from alkash3d.graphics.backend import GraphicsBackend
from alkash3d.utils.logger import logger


class MockBackend(GraphicsBackend):
    """
    Простой «запоминающий» DX12‑бэкенд.
    Он не делает ничего кроме записи вызовов в `self.calls`.
    """
    def __init__(self):
        self.calls = []                     # (method_name, args, kwargs)
        self.device = ctypes.c_void_p(0xBEEF)   # фиктивный дескриптор
        self.command_queue = ctypes.c_void_p(0xCAFE)
        self.swap_chain = ctypes.c_void_p(0xDEAD)
        self.rtv_heap = None
        self.cbv_srv_uav_heap = None
        self._frame_index = 0

    # ------------------- вспомогательные -------------------
    def _record(self, name: str, *a, **kw):
        self.calls.append((name, a, kw))

    # -----------------------------------------------------------------
    # Реализуем всю абстракцию из `GraphicsBackend`
    # (по‑молчанию все методы просто регистрируют свои вызовы)
    # -----------------------------------------------------------------
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        self._record("init_device", hwnd, width, height)

    def resize(self, width: int, height: int) -> None:
        self._record("resize", width, height)

    def present(self) -> None:
        self._record("present")

    # shaders ---------------------------------------------------------
    def compile_shader(self, stage: str, source_path: str) -> Any:
        self._record("compile_shader", stage, source_path)
        # возвращаем «фейковый» blob‑идентификатор
        return 0x11110000 + (0 if stage == "vs" else 0x1)

    def create_graphics_ps(self, vs_blob: Any, ps_blob: Any) -> Any:
        self._record("create_graphics_ps", vs_blob, ps_blob)
        return 0x22220000

    def set_graphics_pipeline(self, pso: Any) -> None:
        self._record("set_graphics_pipeline", pso)

    # buffers ---------------------------------------------------------
    def create_buffer(self, data: bytes, usage: str = "default") -> Any:
        self._record("create_buffer", data, usage)
        # возвращаем «фейковый» указатель
        return ctypes.c_void_p(0xB0B0 + len(data))

    def update_buffer(self, buffer: Any, data: bytes) -> None:
        self._record("update_buffer", buffer, data)

    # textures --------------------------------------------------------
    def create_texture(self,
                      data: bytes | None,
                      width: int,
                      height: int,
                      fmt: str = "RGBA8") -> Any:
        self._record("create_texture", data, width, height, fmt)
        return ctypes.c_void_p(0xC0C0 + width + height)

    def create_constant_buffer(self, data: bytes) -> Any:
        return self.create_buffer(data, usage="constant")

    def update_texture(self,
                       texture: Any,
                       data: bytes,
                       width: int,
                       height: int) -> None:
        self._record("update_texture", texture, data, width, height)

    # descriptor heaps -------------------------------------------------
    def create_descriptor_heap(self,
                               num_descriptors: int,
                               heap_type: str = "cbv_srv_uav") -> Any:
        self._record("create_descriptor_heap", num_descriptors, heap_type)
        # возвращаем «псевдо‑heap» – объект с двумя списками дескрипторов
        class DummyHeap:
            def __init__(self):
                self._next = 0
                self.cpu = {}
                self.gpu = {}

        heap = DummyHeap()
        heap._size = num_descriptors
        self._record("heap_created", heap)
        return heap

    def get_cpu_handle(self, heap: Any, index: int) -> int:
        self._record("get_cpu_handle", heap, index)
        # упрощённый расчёт – просто адрес = base + index * 32
        base = 0x1000
        return base + index * 32

    def get_gpu_handle(self, heap: Any, index: int) -> int:
        self._record("get_gpu_handle", heap, index)
        base = 0x2000
        return base + index * 32

    # root table ------------------------------------------------------
    def set_root_descriptor_table(self, root_index: int, gpu_handle: int) -> None:
        self._record("set_root_descriptor_table", root_index, gpu_handle)

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> None:
        self._record("set_descriptor_heaps", list(heaps))

    # render targets ---------------------------------------------------
    def set_render_target(self, rtv: int) -> None:
        self._record("set_render_target", rtv)

    def set_render_targets(self, rtvs: Sequence[int]) -> None:
        self._record("set_render_targets", list(rtvs))

    def clear_render_target(self,
                            rtv: int,
                            color: Tuple[float, float, float, float]) -> None:
        self._record("clear_render_target", rtv, color)

    # viewport / scissor -----------------------------------------------
    def set_viewport(self,
                     x: int, y: int, width: int, height: int,
                     min_depth: float = 0.0, max_depth: float = 1.0) -> None:
        self._record("set_viewport", x, y, width, height, min_depth, max_depth)

    def set_scissor_rect(self,
                         left: int, top: int,
                         right: int, bottom: int) -> None:
        self._record("set_scissor_rect", left, top, right, bottom)

    # vertex buffers / draw --------------------------------------------
    def set_vertex_buffers(self,
                           vertex_buffer: Any,
                           index_buffer: Any = None) -> None:
        self._record("set_vertex_buffers", vertex_buffer, index_buffer)

    def draw(self,
             vertex_count: int,
             start_vertex: int = 0,
             instance_count: int = 1) -> None:
        self._record("draw", vertex_count, start_vertex, instance_count)

    def draw_indexed(self,
                    index_count: int,
                    start_index: int = 0,
                    base_vertex: int = 0,
                    instance_count: int = 1) -> None:
        self._record("draw_indexed", index_count,
                     start_index, base_vertex, instance_count)

    def draw_fullscreen_quad(self,
                             pso: Any,
                             descriptor_heaps: Sequence[Any],
                             root_parameters: Sequence[Tuple[int, Any]]) -> None:
        self._record("draw_fullscreen_quad", pso, list(descriptor_heaps), list(root_parameters))

    # sync ------------------------------------------------------------
    def wait_for_gpu(self) -> None:
        self._record("wait_for_gpu")

    # resources --------------------------------------------------------
    def release_resource(self, resource: Any) -> None:
        self._record("release_resource", resource)

    # frame management -------------------------------------------------
    def enable_depth_test(self, enable: bool) -> None:
        self._record("enable_depth_test", enable)

    def begin_frame(self) -> None:
        self._record("begin_frame")
        self._frame_index += 1

    def end_frame(self) -> None:
        self._record("end_frame")
        self.present()

    def shutdown(self) -> None:
        self._record("shutdown")

    def get_frame_index(self) -> int:
        return self._frame_index


@pytest.fixture
def mock_backend():
    """Возвращает полностью «моковый» бэкенд."""
    return MockBackend()
