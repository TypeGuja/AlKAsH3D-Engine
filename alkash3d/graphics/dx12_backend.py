# alkash3d/graphics/dx12_backend.py
# -*- coding: utf-8 -*-
"""
Полнофункциональный DirectX 12‑бэкенд.
Все методы, объявленные в `GraphicsBackend`, реализованы.
"""

from __future__ import annotations

import ctypes
import os
import time
import traceback
from typing import Any, Sequence, Tuple, Optional  # <-- Optional нужен

from alkash3d.graphics.backend import GraphicsBackend
from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap
from alkash3d.utils.logger import logger

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print("[DX12_BACKEND]", *args, **kwargs)


class DX12Texture:
    """Обёртка над ID3D12Resource*."""
    __slots__ = ("ptr", "_srv_gpu", "width", "height", "format")

    def __init__(self, ptr: ctypes.c_void_p, width: int = 0, height: int = 0, fmt: str = ""):
        self.ptr = ptr
        self._srv_gpu: Optional[int] = None
        self.width = width
        self.height = height
        self.format = fmt


class DX12Backend(GraphicsBackend):
    """DirectX 12‑бэкенд, полностью реализующий `GraphicsBackend`."""

    # ------------------------------------------------------------------
    # Инициализация
    # ------------------------------------------------------------------
    def __init__(self) -> None:
        debug_print("DX12Backend.__init__()")
        self.device: Optional[ctypes.c_void_p] = None
        self.command_queue: Optional[ctypes.c_void_p] = None
        self.swap_chain: Optional[ctypes.c_void_p] = None

        self.viewport: Tuple[int, int, int, int] = (0, 0, 0, 0)
        self.scissor: Tuple[int, int, int, int] = (0, 0, 0, 0)

        self.rtv_heap: Optional[DescriptorHeap] = None
        self.cbv_srv_uav_heap: Optional[DescriptorHeap] = None

        self._rtv_cpu_handles: list[int] = []
        self._resources: list[Any] = []
        self._released_resources = set()
        self._depth_test_enabled: bool = False
        self._in_stub_mode: bool = False

        self._hwnd: int = 0
        self._width: int = 0
        self._height: int = 0
        self._vsync_enabled: bool = True

        self._constant_buffers: set[int] = set()
        self._current_frame = 0
        self._initialized = False

    # ------------------------------------------------------------------
    # Внутренние вспомогательные функции
    # ------------------------------------------------------------------
    def _reset_viewport_and_scissor(self, w: int, h: int) -> bool:
        """Устанавливает Viewport/Scissor (или просто True в stub‑режиме)."""
        debug_print(f"_reset_viewport_and_scissor({w}x{h})")
        self.viewport = (0, 0, w, h)
        self.scissor = (0, 0, w, h)

        if self._in_stub_mode:
            return True

        try:
            if not self.set_viewport(0, 0, w, h):
                return False
            if not self.set_scissor_rect(0, 0, w, h):
                return False
            return True
        except Exception as e:
            debug_print(f"Viewport/scissor reset error: {e}")
            return False

    def _get_swapchain_buffer(self, idx: int) -> Optional[ctypes.c_void_p]:
        if not self.swap_chain:
            return None
        try:
            return dx.swap_chain_get_buffer(self.swap_chain, idx)
        except Exception as e:
            debug_print(f"swap_chain_get_buffer({idx}) error: {e}")
            return None

    def _create_swapchain_rtv(self) -> bool:
        """Создаёт RTV‑дескрипторы для всех буферов swap‑chain."""
        debug_print("_create_swapchain_rtv()")
        if not self.swap_chain or not self.rtv_heap:
            debug_print("  No swap chain / RTV heap → abort")
            return False

        self._rtv_cpu_handles.clear()
        self.rtv_heap.reset()

        for i in range(dx.SWAP_CHAIN_BUFFER_COUNT):
            buf = self._get_swapchain_buffer(i)
            if not buf:
                debug_print(f"  Buffer {i} is None → skip")
                continue

            idx = self.rtv_heap.next_free()
            cpu = self.rtv_heap.get_cpu_handle(idx)

            if not dx.create_render_target_view(self.device, buf, cpu):
                debug_print(f"  RTV creation failed for buffer {i}")
                continue

            self._rtv_cpu_handles.append(cpu)

        debug_print(f"  Created {len(self._rtv_cpu_handles)} RTV(s)")
        return len(self._rtv_cpu_handles) > 0

    def _cleanup_resources(self) -> None:
        """Освобождает всё, что было создано ранее."""
        debug_print("_cleanup_resources()")
        try:
            self.wait_for_gpu()
        except Exception:
            pass

        for res in self._resources:
            try:
                self.release_resource(res)
            except Exception:
                pass
        self._resources.clear()

        self.rtv_heap = None
        self.cbv_srv_uav_heap = None
        self._rtv_cpu_handles.clear()

        for attr in ("swap_chain", "command_queue", "device"):
            obj = getattr(self, attr)
            if obj:
                try:
                    self.release_resource(obj)
                except Exception:
                    pass
                setattr(self, attr, None)

        try:
            dx.force_cleanup()
        except Exception:
            pass

        time.sleep(0.1)
        debug_print("  cleanup finished")

    # ------------------------------------------------------------------
    # Реализация методов GraphicsBackend
    # ------------------------------------------------------------------
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        debug_print(f"init_device(hwnd={hex(hwnd)}, {width}x{height})")
        self._hwnd = hwnd
        self._width = width
        self._height = height
        self._initialized = False

        self._cleanup_resources()

        try:
            # 1️⃣ Device
            dev = dx.create_device()
            if not dev or not dev.value:
                raise RuntimeError("Failed to create DX12 device")
            self.device = dev
            debug_print(f"  Device ptr = {hex(self.device.value)}")

            # 2️⃣ Command queue
            cq = dx.create_command_queue(self.device)
            if not cq or not cq.value:
                raise RuntimeError("Failed to create command queue")
            self.command_queue = cq
            debug_print(f"  Queue ptr = {hex(self.command_queue.value)}")

            # 3️⃣ Swap chain (если передан HWND)
            if hwnd:
                sc = dx.create_swap_chain(self.command_queue, hwnd, width, height)
                if sc and sc.value:
                    self.swap_chain = sc
                    debug_print(f"  SwapChain ptr = {hex(self.swap_chain.value)}")
                else:
                    debug_print("  Swap chain creation failed – stub mode")
            else:
                debug_print("  No HWND supplied → headless mode")

            # 4️⃣ Viewport / Scissor
            self._reset_viewport_and_scissor(width, height)

            # 5️⃣ Descriptor‑heaps
            self.rtv_heap = DescriptorHeap(
                device=self.device,
                num_descriptors=dx.SWAP_CHAIN_BUFFER_COUNT + 4,
                heap_type="rtv",
                shader_visible=False,
            )
            debug_print("  RTV heap created")

            self.cbv_srv_uav_heap = DescriptorHeap(
                device=self.device,
                num_descriptors=1024,
                heap_type="cbv_srv_uav",
                shader_visible=True,
            )
            debug_print("  CBV/SRV/UAV heap created")

            # 6️⃣ RTV‑дескрипторы (если есть swap‑chain)
            if self.swap_chain:
                if not self._create_swapchain_rtv():
                    raise RuntimeError("Failed to create RTVs")

            self._in_stub_mode = False
            self._initialized = True
            self._current_frame = self.get_frame_index()
            logger.info("[DX12Backend] Device initialised successfully")
        except Exception as e:
            logger.error(f"[DX12Backend] init_device failed: {e}")
            traceback.print_exc()
            self._in_stub_mode = True
            self.device = ctypes.c_void_p(0xDEADBEEF)
            self._initialized = False

    # ------------------------------------------------------------------
    # Resize / present
    # ------------------------------------------------------------------
    def resize(self, width: int, height: int) -> bool:
        debug_print(f"resize({width}x{height})")
        self._width = width
        self._height = height
        self._reset_viewport_and_scissor(width, height)

        if not self._in_stub_mode and self.swap_chain:
            if not dx.resize_swap_chain(self.swap_chain, width, height):
                return False
            if self.rtv_heap:
                return self._create_swapchain_rtv()
        return False

    def present(self, sync_interval: int = 1) -> bool:
        debug_print(f"present(sync={sync_interval})")
        if not self._initialized or self._in_stub_mode or not self.swap_chain:
            return False
        try:
            ok = dx.present_swap_chain(self.swap_chain, sync_interval)
            if ok:
                self._current_frame = (self._current_frame + 1) % dx.SWAP_CHAIN_BUFFER_COUNT
                if self._hwnd:
                    ctypes.windll.user32.InvalidateRect(self._hwnd, None, True)
                    ctypes.windll.user32.UpdateWindow(self._hwnd)
            return ok
        except Exception as e:
            logger.error(f"[DX12Backend] present failed: {e}")
            return False

    def set_vsync(self, enable: bool) -> None:
        self._vsync_enabled = enable

    # ------------------------------------------------------------------
    # Шейдеры
    # ------------------------------------------------------------------
    def compile_shader(
        self,
        shader_type: str,
        source_path: str,
        entry_point: Optional[str] = None,
    ) -> int:
        """
        Compile a HLSL shader.

        Parameters
        ----------
        shader_type : "vs" | "ps"
            Тип шейдера.
        source_path : str
            Полный путь к *.hlsl* файлу.
        entry_point : str, optional
            Имя функции‑входа. Если не передано, используется историческое
            имя: ``VSMain`` для вершинного шейдера и ``PSMain`` для пиксельного.
        """
        if self._in_stub_mode:
            return 0x12345678

        # Если пользователь не указал entry_point, берём «старый» fallback.
        if entry_point is None:
            entry_point = "VSMain" if shader_type == "vs" else "PSMain"

        profile = "vs_5_0" if shader_type == "vs" else "ps_5_0"

        if not os.path.isfile(source_path):
            logger.error(f"Shader file not found: {source_path}")
            return 0x12345678

        return dx.compile_shader(source_path, entry_point, profile)

    # ------------------------------------------------------------------
    # PSO
    # ------------------------------------------------------------------
    def create_graphics_ps(self, vs_blob: int, ps_blob: int) -> Optional[int]:
        """Создаёт PSO из двух скомпилированных шейдер‑blob‑ов.

        `vs_blob` и `ps_blob` – уже‑целочисленные дескрипторы,
        `dx.create_graphics_ps` их сам преобразует.
        """
        if self._in_stub_mode or not vs_blob or not ps_blob:
            return None

        # Передаём int‑значения напрямую – обёртка уже делает `_to_cvoid`.
        return dx.create_graphics_ps(self.device, vs_blob, ps_blob)

    def set_graphics_pipeline(self, pso: int) -> bool:
        if self._in_stub_mode or not pso:
            return False
        return dx.set_graphics_pipeline(ctypes.c_void_p(pso))

    # ------------------------------------------------------------------
    # Буферы / констант‑буферы
    # ------------------------------------------------------------------
    def create_buffer(self, data: bytes, usage: str = "default") -> Optional[ctypes.c_void_p]:
        if self._in_stub_mode or not self.device:
            return ctypes.c_void_p(0xDEADBEEF)

        if not self.device.value:
            logger.error("[DX12Backend] Device is null")
            return None

        # Create buffer
        buf = dx.create_buffer(self.device, len(data), usage)
        if not buf or not buf.value:
            logger.error("[DX12Backend] Failed to create buffer")
            return None

        # Update buffer data
        if data and len(data) > 0:
            if not self.update_buffer(buf, data):
                logger.error("[DX12Backend] Failed to update buffer data")
                # Still return the buffer, but log error

        self._resources.append(buf)
        return buf

    def update_buffer(self, buffer: ctypes.c_void_p, data: bytes) -> bool:
        if self._in_stub_mode:
            return False

        if not buffer or not buffer.value:
            logger.error("[DX12Backend] update_buffer: buffer is null")
            return False

        if not data:
            logger.error("[DX12Backend] update_buffer: data is empty")
            return False

        # Verify data is valid
        try:
            # Just check we can access the data
            _ = len(data)
            _ = data[0] if data else None
        except Exception as e:
            logger.error(f"[DX12Backend] update_buffer: invalid data: {e}")
            return False

        return dx.update_subresource(buffer, data)

    def create_constant_buffer(self, data: bytes) -> Any:
        """Утилита‑обёртка – просто переадресует в `create_buffer`."""
        return self.create_buffer(data, usage="constant")

    # ------------------------------------------------------------------
    # Текстуры
    # ------------------------------------------------------------------
    def create_texture(self, data: Optional[bytes], w: int, h: int, fmt: str = "RGBA8") -> Optional[DX12Texture]:
        if self._in_stub_mode or not self.device:
            return DX12Texture(ctypes.c_void_p(0xDEADBEEF), w, h, fmt)

        tex_ptr = dx.create_texture_from_memory(self.device, data, w, h, fmt.encode())
        if not tex_ptr or not tex_ptr.value:
            logger.error("create_texture_from_memory returned NULL")
            return None

        tex = DX12Texture(tex_ptr, w, h, fmt)
        self._resources.append(tex_ptr)
        return tex

    def update_texture(self, texture: DX12Texture, data: bytes, w: int, h: int) -> bool:
        if self._in_stub_mode or not texture or not texture.ptr:
            return False
        return dx.update_texture(texture.ptr, data, w, h)

    # ------------------------------------------------------------------
    # Descriptor‑heaps
    # ------------------------------------------------------------------
    def create_descriptor_heap(
        self,
        num_descriptors: int,
        heap_type: str = "cbv_srv_uav",
        shader_visible: bool = True,
    ) -> Optional[DescriptorHeap]:
        if self._in_stub_mode or not self.device:
            return None

        heap_num = {"rtv": 0, "dsv": 1, "cbv_srv_uav": 2}.get(heap_type.lower(), 2)
        heap_ptr = dx.create_descriptor_heap(self.device, num_descriptors, heap_num, shader_visible)
        if not heap_ptr or not heap_ptr.value:
            return None

        heap = DescriptorHeap(self.device, num_descriptors, heap_type, shader_visible)
        heap.heap_ptr = heap_ptr
        return heap

    def get_cpu_handle(self, heap: DescriptorHeap, index: int) -> int:
        return heap.get_cpu_handle(index)

    def get_gpu_handle(self, heap: DescriptorHeap, index: int) -> int:
        return heap.get_gpu_handle(index)

    # ------------------------------------------------------------------
    # Views
    # ------------------------------------------------------------------
    def create_shader_resource_view(self, texture: DX12Texture, cpu_handle: int) -> bool:
        if self._in_stub_mode or not texture or not texture.ptr:
            return False
        return dx.create_shader_resource_view(self.device, texture.ptr, cpu_handle)

    def create_render_target_view(self, texture: DX12Texture, cpu_handle: int) -> bool:
        if self._in_stub_mode or not texture or not texture.ptr:
            return False
        return dx.create_render_target_view(self.device, texture.ptr, cpu_handle)

    def create_constant_buffer_view(self, resource: ctypes.c_void_p, cpu_handle: int) -> bool:
        if self._in_stub_mode or not resource:
            return False
        return dx.create_constant_buffer_view(self.device, resource, cpu_handle)

    # ------------------------------------------------------------------
    # Render‑commands
    # ------------------------------------------------------------------
    def set_root_descriptor_table(self, root_index: int, gpu_handle: int) -> bool:
        if self._in_stub_mode:
            return False
        return dx.set_root_descriptor_table(root_index, gpu_handle)

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> bool:
        if self._in_stub_mode:
            return False
        return dx.set_descriptor_heaps(heaps)

    def set_render_target(self, rtv: int) -> bool:
        if self._in_stub_mode:
            return False
        return dx.set_render_target(rtv)

    def set_render_targets(self, rtvs: Sequence[int]) -> bool:
        if self._in_stub_mode:
            return False
        return dx.set_render_targets(rtvs)

    def clear_render_target(self, rtv: int, color: Tuple[float, float, float, float]) -> bool:
        if self._in_stub_mode:
            return False
        return dx.clear_render_target(rtv, color)

    def set_viewport(self, x: int, y: int, w: int, h: int,
                     min_depth: float = 0.0, max_depth: float = 1.0) -> bool:
        if self._in_stub_mode:
            return False
        self.viewport = (x, y, w, h)
        return dx.set_viewport(x, y, w, h, min_depth, max_depth)

    def set_scissor_rect(self, left: int, top: int, right: int, bottom: int) -> bool:
        if self._in_stub_mode:
            return False
        self.scissor = (left, top, right, bottom)
        return dx.set_scissor_rect(left, top, right, bottom)

    def set_vertex_buffers(self,
                          vertex_buffer: ctypes.c_void_p,
                          index_buffer: Optional[ctypes.c_void_p] = None) -> bool:
        if self._in_stub_mode:
            return False
        return dx.set_vertex_buffers(vertex_buffer, index_buffer)

    def draw(self, vertex_count: int, start_vertex: int = 0,
             instance_count: int = 1) -> bool:
        if self._in_stub_mode:
            return False
        return dx.draw_instanced(vertex_count, instance_count,
                                 start_vertex, 0)

    def draw_indexed(self,
                     index_count: int,
                     start_index: int = 0,
                     base_vertex: int = 0,
                     instance_count: int = 1) -> bool:
        if self._in_stub_mode:
            return False
        return dx.draw_indexed_instanced(index_count,
                                        instance_count,
                                        start_index,
                                        base_vertex,
                                        0)

    def draw_fullscreen_quad(self,
                             pso: Any,
                             descriptor_heaps: Sequence[Any],
                             root_parameters: Sequence[Tuple[int, int]]) -> None:
        """Рисует один fullscreen‑треугольник."""
        if self._in_stub_mode:
            return

        if not self.set_graphics_pipeline(pso):
            logger.error("[DX12Backend] draw_fullscreen_quad: set_graphics_pipeline failed")
            return

        if not self.set_descriptor_heaps(descriptor_heaps):
            logger.error("[DX12Backend] draw_fullscreen_quad: set_descriptor_heaps failed")
            return

        for slot, gpu_handle in root_parameters:
            if not self.set_root_descriptor_table(slot, gpu_handle):
                logger.error(f"[DX12Backend] draw_fullscreen_quad: set_root_descriptor_table({slot}) failed")
                return

        # Один треугольник покрывает весь экран
        self.draw(3, start_vertex=0, instance_count=1)

    # ------------------------------------------------------------------
    # Frame control
    # ------------------------------------------------------------------
    def begin_frame(self) -> bool:
        if not self._initialized or self._in_stub_mode:
            return False
        if self.rtv_heap:
            self.rtv_heap.reset()
        if self.cbv_srv_uav_heap:
            self.cbv_srv_uav_heap.reset()
        return dx.begin_frame()

    def end_frame(self) -> bool:
        if not self._initialized or self._in_stub_mode:
            return False
        ok = dx.end_frame()
        if ok and self.swap_chain:
            # V‑sync контролируется в `present`
            self.present(1 if self._vsync_enabled else 0)
        return ok

    def wait_for_gpu(self) -> bool:
        if not self.command_queue:
            return False
        return dx.wait_for_gpu()

    # ------------------------------------------------------------------
    # Misc
    # ------------------------------------------------------------------
    def enable_depth_test(self, enable: bool) -> None:
        self._depth_test_enabled = enable

    def release_resource(self, resource: Any) -> None:
        if not resource:
            return
        ptr = getattr(resource, "value", None) or int(resource)
        stub = {0xDEADBEEF, 0xDEADF00D, 0xFEEDC0DE, 0x12345678, 0x87654321}
        if ptr in stub or ptr in self._released_resources:
            return
        try:
            self._released_resources.add(ptr)
            dx.release_resource(resource)
        except Exception as e:
            logger.error(f"release_resource error: {e}")
            self._released_resources.discard(ptr)

    def shutdown(self) -> None:
        logger.info("[DX12Backend] shutdown")
        try:
            self.end_frame()
        except Exception:
            pass
        for res in self._resources:
            try:
                self.release_resource(res)
            except Exception:
                pass
        self._resources.clear()
        try:
            dx.force_cleanup()
        except Exception:
            pass
        self._in_stub_mode = True

    def get_frame_index(self) -> int:
        if self._initialized and not self._in_stub_mode:
            try:
                return dx.get_frame_index()
            except Exception:
                pass
        return self._current_frame

    def get_rtv_descriptor_size(self) -> int:
        return dx.get_rtv_descriptor_size()

    def get_dsv_descriptor_size(self) -> int:
        return dx.get_dsv_descriptor_size()
