# -*- coding: utf-8 -*-
"""
conftest.py – мок‑бэкенд, покрывающий весь движок.
Не требует реального DirectX 12‑модуля, а проверяет, что Engine →
Renderer → Backend вызывают ожидаемые методы.
"""

import ctypes
from typing import Any, Sequence, Tuple, Optional
import pytest

from alkash3d.graphics.backend import GraphicsBackend
from alkash3d.utils.logger import logger


# ----------------------------------------------------------------------
# Dummy descriptor‑heap, который умеет выделять дескрипторы
# ----------------------------------------------------------------------
class _DummyHeap:
    """
    Минимальная имитация DescriptorHeap.
    Методы `next_free`, `get_cpu_handle`, `get_gpu_handle`
    возвращают просто посчитанные целочисленные “хэндлы”.
    """
    def __init__(self, heap_type: str, size: int):
        self.heap_type = heap_type          # «rtv» или «cbv_srv_uav»
        self._size = size
        self._next = 0

    # --------------------------------------------------------------
    def next_free(self) -> int:
        """Вернуть индекс следующего свободного дескриптора."""
        if self._next >= self._size:
            raise RuntimeError(f"{self.heap_type.upper()} heap exhausted")
        idx = self._next
        self._next += 1
        return idx

    # --------------------------------------------------------------
    def get_cpu_handle(self, idx: int) -> int:
        """Базовый CPU‑хэндл: 0x1000 для RTV, 0x2000 для CBV/SRV/UAV."""
        base = 0x1000 if self.heap_type == "rtv" else 0x2000
        return base + idx * 0x20          # 0x20 = 32 байта (типичный размер дескриптора)

    # --------------------------------------------------------------
    def get_gpu_handle(self, idx: int) -> int:
        """Базовый GPU‑хэндл: 0x3000 для RTV, 0x4000 для CBV/SRV/UAV."""
        base = 0x3000 if self.heap_type == "rtv" else 0x4000
        return base + idx * 0x20


# ----------------------------------------------------------------------
# MockBackend – полностью реализует интерфейс GraphicsBackend.
# ----------------------------------------------------------------------
class MockBackend(GraphicsBackend):
    """
    Минимальная имитация DX12‑бэкенда.
    Каждый метод только записывает вызов в `self.calls`.
    """

    def __init__(self) -> None:
        # (method_name, args, kwargs)
        self.calls: list[Tuple[str, Tuple[Any, ...], dict]] = []

        # Фиктивные дескрипторы устройства / queue / swap‑chain
        self.device = ctypes.c_void_p(0xBEEF)
        self.command_queue = ctypes.c_void_p(0xCAFE)
        self.swap_chain = ctypes.c_void_p(0xDEAD)

        # Создаём два «реальных» хипа, чтобы ForwardRenderer и Shader
        # могли пользоваться ими без дополнительных проверок.
        self.rtv_heap = _DummyHeap("rtv", size=2)                # 2‑descriptor RTV‑heap
        self.cbv_srv_uav_heap = _DummyHeap("cbv_srv_uav", size=1024)

        # Кадровый счётчик (для get_frame_index)
        self._frame_index: int = 0

        # Список ресурсов – нужен только чтобы не терять ссылки в shutdown()
        self._resources: list[Any] = []

    # -----------------------------------------------------------------
    # Вспомогательная запись вызова
    # -----------------------------------------------------------------
    def _record(self, name: str, *a, **kw) -> None:
        self.calls.append((name, a, kw))

    # -----------------------------------------------------------------
    # Device / swap‑chain -------------------------------------------------
    # -----------------------------------------------------------------
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        self._record("init_device", hwnd, width, height)

    def resize(self, width: int, height: int) -> None:
        self._record("resize", width, height)

    def present(self) -> None:
        self._record("present")

    # -----------------------------------------------------------------
    # Shaders -----------------------------------------------------------
    # -----------------------------------------------------------------
    def compile_shader(self, stage: str, source_path: str) -> Any:
        self._record("compile_shader", stage, source_path)
        # Возврат «фейкового» blob‑идентификатора
        return 0x11110000 + (0 if stage == "vs" else 0x1)

    def create_graphics_ps(self, vs_blob: Any, ps_blob: Any) -> Any:
        self._record("create_graphics_ps", vs_blob, ps_blob)
        return 0x22220000

    def set_graphics_pipeline(self, pso: Any) -> None:
        self._record("set_graphics_pipeline", pso)

    # -----------------------------------------------------------------
    # Buffers -----------------------------------------------------------
    # -----------------------------------------------------------------
    def create_buffer(self, data: bytes, usage: str = "default") -> Any:
        self._record("create_buffer", data, usage)
        ptr = ctypes.c_void_p(0xB0B0 + len(data))
        self._resources.append(ptr)
        return ptr

    def update_buffer(self, buffer: Any, data: bytes) -> None:
        self._record("update_buffer", buffer, data)

    # -----------------------------------------------------------------
    # Textures ----------------------------------------------------------
    # -----------------------------------------------------------------
    def create_texture(
        self,
        data: bytes | None,
        width: int,
        height: int,
        fmt: str = "RGBA8",
    ) -> Any:
        self._record("create_texture", data, width, height, fmt)
        tex = ctypes.c_void_p(0xC0C0 + width + height)
        self._resources.append(tex)
        return tex

    def create_constant_buffer(self, data: bytes) -> Any:
        # Просто переиспользуем create_buffer с «constant»‑usage
        return self.create_buffer(data, usage="constant")

    def update_texture(
        self,
        texture: Any,
        data: bytes,
        width: int,
        height: int,
    ) -> None:
        self._record("update_texture", texture, data, width, height)

    # -----------------------------------------------------------------
    # Descriptor heaps ----------------------------------------------------
    # -----------------------------------------------------------------
    def create_descriptor_heap(
        self,
        num_descriptors: int,
        heap_type: str = "cbv_srv_uav",
    ) -> Any:
        self._record("create_descriptor_heap", num_descriptors, heap_type)
        # Мы **не** создаём новый heap – возвращаем уже готовый:
        if heap_type == "rtv":
            return self.rtv_heap
        if heap_type == "cbv_srv_uav":
            return self.cbv_srv_uav_heap
        raise ValueError(f"Unsupported heap type: {heap_type}")

    def get_cpu_handle(self, heap: Any, index: int) -> int:
        self._record("get_cpu_handle", heap, index)
        return heap.get_cpu_handle(index)

    def get_gpu_handle(self, heap: Any, index: int) -> int:
        self._record("get_gpu_handle", heap, index)
        return heap.get_gpu_handle(index)

    # -----------------------------------------------------------------
    # SRV / RTV creation (заглушки) --------------------------------------
    # -----------------------------------------------------------------
    def create_shader_resource_view(self, resource: Any, cpu_handle) -> None:
        self._record("create_shader_resource_view", resource, cpu_handle)

    def create_render_target_view(self, resource: Any, cpu_handle) -> None:
        self._record("create_render_target_view", resource, cpu_handle)

    # -----------------------------------------------------------------
    # Root‑descriptor‑table ------------------------------------------------
    # -----------------------------------------------------------------
    def set_root_descriptor_table(self, root_index: int, gpu_handle: Any) -> None:
        self._record("set_root_descriptor_table", root_index, gpu_handle)

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> None:
        self._record("set_descriptor_heaps", list(heaps))

    # -----------------------------------------------------------------
    # Render‑targets ------------------------------------------------------
    # -----------------------------------------------------------------
    def set_render_target(self, rtv: Any) -> None:
        self._record("set_render_target", rtv)

    def set_render_targets(self, rtvs: Sequence[Any]) -> None:
        self._record("set_render_targets", list(rtvs))

    def clear_render_target(
        self,
        rtv: Any,
        color: Tuple[float, float, float, float] = (0.0, 0.0, 0.0, 1.0),
    ) -> None:
        self._record("clear_render_target", rtv, color)

    # -----------------------------------------------------------------
    # Viewport / Scissor --------------------------------------------------
    # -----------------------------------------------------------------
    def set_viewport(
        self,
        x: int,
        y: int,
        width: int,
        height: int,
        min_depth: float = 0.0,
        max_depth: float = 1.0,
    ) -> None:
        self._record(
            "set_viewport", x, y, width, height, min_depth, max_depth
        )

    def set_scissor_rect(
        self,
        left: int,
        top: int,
        right: int,
        bottom: int,
    ) -> None:
        self._record("set_scissor_rect", left, top, right, bottom)

    # -----------------------------------------------------------------
    # Vertex / Index buffers & draw calls ---------------------------------
    # -----------------------------------------------------------------
    def set_vertex_buffers(
        self,
        vertex_buffer: Any,
        index_buffer: Optional[Any] = None,
    ) -> None:
        self._record("set_vertex_buffers", vertex_buffer, index_buffer)

    def draw(
        self,
        vertex_count: int,
        start_vertex: int = 0,
        instance_count: int = 1,
    ) -> None:
        self._record("draw", vertex_count, start_vertex, instance_count)

    def draw_indexed(
        self,
        index_count: int,
        start_index: int = 0,
        base_vertex: int = 0,
        instance_count: int = 1,
    ) -> None:
        self._record(
            "draw_indexed",
            index_count,
            start_index,
            base_vertex,
            instance_count,
        )

    def draw_fullscreen_quad(
        self,
        pso: Any,
        descriptor_heaps: Sequence[Any],
        root_parameters: Sequence[Tuple[int, Any]],
    ) -> None:
        self._record(
            "draw_fullscreen_quad",
            pso,
            list(descriptor_heaps),
            list(root_parameters),
        )

    # -----------------------------------------------------------------
    # Sync / Release ------------------------------------------------------
    # -----------------------------------------------------------------
    def wait_for_gpu(self) -> None:
        self._record("wait_for_gpu")

    def release_resource(self, resource: Any) -> None:
        self._record("release_resource", resource)

    # -----------------------------------------------------------------
    # Frame management ----------------------------------------------------
    # -----------------------------------------------------------------
    def enable_depth_test(self, enable: bool) -> None:
        self._record("enable_depth_test", enable)

    def begin_frame(self) -> None:
        self._record("begin_frame")
        self._frame_index += 1

    def end_frame(self) -> None:
        self._record("end_frame")
        # В настоящем бекенде здесь вызывается present().
        # Для мок‑бэкенда делаем отдельный вызов в конце.
        self.present()

    def shutdown(self) -> None:
        self._record("shutdown")
        self._resources.clear()

    # -----------------------------------------------------------------
    # Информационные методы -----------------------------------------------
    # -----------------------------------------------------------------
    def get_frame_index(self) -> int:
        return self._frame_index

    def get_rtv_descriptor_size(self) -> int:
        return 32

    def get_dsv_descriptor_size(self) -> int:
        return 32

    # -----------------------------------------------------------------
    # Утилиты для тестов --------------------------------------------------
    # -----------------------------------------------------------------
    def called(self, name: str) -> bool:
        """True, если метод `name` был вызван хотя бы один раз."""
        return any(call[0] == name for call in self.calls)

    def count(self, name: str) -> int:
        """Сколько раз был вызван метод `name`."""
        return sum(1 for call in self.calls if call[0] == name)


# ----------------------------------------------------------------------
# PyTest‑fixture – возвращает новый мок‑бэкенд для каждого теста
# ----------------------------------------------------------------------
@pytest.fixture
def mock_backend() -> MockBackend:
    """Создаёт чистый MockBackend."""
    return MockBackend()
