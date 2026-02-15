# -*- coding: utf-8 -*-
"""
Полнофункциональный DirectX 12‑бэкенд.
* При отсутствии нативной DLL переходим в stub‑режим.
* Добавлен метод `recreate_swapchain_rtv()` – заново создает RTV‑дескрипторы
  после замены `rtv_heap`.
* `create_texture()` теперь **не вызывает `Map`** для ресурсов в `DEFAULT`‑heap – данные копируются через `dx.update_texture`,
  тем самым устраняя ошибку `HRESULT 0x80070057`.
"""

from __future__ import annotations

import ctypes
import os
from typing import Any, Sequence, Tuple, Optional

from alkash3d.graphics.backend import GraphicsBackend
from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap
from alkash3d.utils.logger import logger

class DX12Texture:
    """Обёртка над ID3D12Resource*."""
    __slots__ = ("ptr", "_srv_gpu")

    def __init__(self, ptr: ctypes.c_void_p):
        self.ptr = ptr
        self._srv_gpu = None

class DX12Backend(GraphicsBackend):
    """DirectX 12‑бэкенд с автоматическим переходом в stub‑режим."""

    def __init__(self) -> None:
        self.device: Optional[ctypes.c_void_p] = None
        self.command_queue: Optional[ctypes.c_void_p] = None
        self.swap_chain: Optional[ctypes.c_void_p] = None

        self.viewport: Tuple[int, int, int, int] = (0, 0, 0, 0)
        self.scissor: Tuple[int, int, int, int] = (0, 0, 0, 0)

        self.rtv_heap: Optional[DescriptorHeap] = None
        self.cbv_srv_uav_heap: Optional[DescriptorHeap] = None

        self._rtv_cpu_handles: list[int] = []
        self._resources: list[Any] = []
        self._depth_test_enabled: bool = False
        self._in_stub_mode: bool = False

        self._hwnd: int = 0
        self._width: int = 0
        self._height: int = 0

    # -----------------------------------------------------------------
    #   Private helpers
    # -----------------------------------------------------------------
    def _reset_viewport_and_scissor(self, w: int, h: int) -> None:
        self.viewport = (0, 0, w, h)
        self.scissor = (0, 0, w, h)

        if not self._in_stub_mode:
            self.set_viewport(0, 0, w, h)
            self.set_scissor_rect(0, 0, w, h)

    def _create_swapchain_rtv(self) -> None:
        """Создать RTV‑дескрипторы для back‑buffer‑ов swap‑chain."""
        if not self.swap_chain or not self.swap_chain.value:
            logger.debug("[DX12Backend] No swap chain – RTV creation skipped")
            return

        self._rtv_cpu_handles.clear()

        for i in range(dx.SWAP_CHAIN_BUFFER_COUNT):
            back_buf = dx.swap_chain_get_buffer(self.swap_chain, i)
            if not back_buf or not back_buf.value:
                logger.error(f"[DX12Backend] GetBuffer({i}) failed")
                continue

            rtv_idx = self.rtv_heap.next_free()
            cpu_handle = self.rtv_heap.get_cpu_handle(rtv_idx)
            self.create_render_target_view(back_buf, cpu_handle)
            self._rtv_cpu_handles.append(cpu_handle)

        logger.debug(f"[DX12Backend] Created {len(self._rtv_cpu_handles)} RTV(s)")

    # -----------------------------------------------------------------
    #   Public API – device / swap‑chain creation
    # -----------------------------------------------------------------
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        logger.info("[DX12Backend] Initialising DirectX 12 device")
        self._hwnd = hwnd
        self._width = width
        self._height = height

        try:
            device_ptr = dx.create_device()
            if not device_ptr or not device_ptr.value:
                raise RuntimeError("Failed to create device – null pointer")
            self.device = device_ptr
            logger.debug(f"[DX12Backend] Device created: {hex(self.device.value)}")

            queue_ptr = dx.create_command_queue(self.device)
            if not queue_ptr or not queue_ptr.value:
                raise RuntimeError("Failed to create command queue")
            self.command_queue = queue_ptr
            logger.debug(f"[DX12Backend] Command queue created: {hex(self.command_queue.value)}")

            if hwnd != 0:
                swap_ptr = dx.create_swap_chain(
                    self.command_queue, hwnd, width, height
                )
                if not swap_ptr or not swap_ptr.value:
                    logger.warning("[DX12Backend] Swap chain creation failed")
                    self.swap_chain = None
                else:
                    self.swap_chain = swap_ptr
                    logger.debug(f"[DX12Backend] Swap chain created: {hex(self.swap_chain.value)}")
            else:
                self.swap_chain = None
                logger.debug("[DX12Backend] No HWND supplied – swap chain disabled")

            self._reset_viewport_and_scissor(width, height)

            self.rtv_heap = DescriptorHeap(
                device=self.device,
                num_descriptors=dx.SWAP_CHAIN_BUFFER_COUNT + 1,
                heap_type="rtv",
            )
            logger.debug("[DX12Backend] RTV heap created")

            try:
                self.cbv_srv_uav_heap = DescriptorHeap(
                    device=self.device,
                    num_descriptors=1024,
                    heap_type="cbv_srv_uav",
                )
                logger.debug("[DX12Backend] CBV/SRV/UAV heap (1024) created")
            except Exception as e:
                logger.warning(f"[DX12Backend] 1024‑descriptor heap failed ({e}); trying 256")
                try:
                    self.cbv_srv_uav_heap = DescriptorHeap(
                        device=self.device,
                        num_descriptors=256,
                        heap_type="cbv_srv_uav",
                    )
                    logger.debug("[DX12Backend] CBV/SRV/UAV heap (256) created")
                except Exception as e2:
                    logger.error(f"[DX12Backend] Even 256‑descriptor heap failed ({e2}) – stub mode")
                    self.cbv_srv_uav_heap = None

            if self.rtv_heap and self.swap_chain:
                self._create_swapchain_rtv()
            else:
                logger.debug("[DX12Backend] Skipping RTV creation (no swap chain)")

            self._in_stub_mode = False
            logger.info("[DX12Backend] Device initialised successfully")
        except Exception as e:
            logger.error(f"[DX12Backend] Device initialisation failed: {e}")
            logger.warning("[DX12Backend] Switching to STUB mode")
            self._in_stub_mode = True
            self.device = ctypes.c_void_p(0xDEADBEEF)

    # -----------------------------------------------------------------
    #   Resize
    # -----------------------------------------------------------------
    def resize(self, width: int, height: int) -> None:
        logger.info(f"[DX12Backend] Resize {width}x{height}")
        self._reset_viewport_and_scissor(width, height)

        if not self._in_stub_mode and self.swap_chain and self.swap_chain.value:
            try:
                dx.resize_swap_chain(self.swap_chain, width, height)
                if self.rtv_heap:
                    self._create_swapchain_rtv()
            except Exception as e:
                logger.error(f"[DX12Backend] Resize failed: {e}")
                self._in_stub_mode = True

    # -----------------------------------------------------------------
    #   Present
    # -----------------------------------------------------------------
    def present(self) -> None:
        if not self._in_stub_mode and self.swap_chain and self.swap_chain.value:
            try:
                dx.present_swap_chain(self.swap_chain, sync_interval=1)
            except Exception as e:
                logger.error(f"[DX12Backend] Present failed: {e}")
                self._in_stub_mode = True

    # -----------------------------------------------------------------
    #   Shaders
    # -----------------------------------------------------------------
    def compile_shader(self, shader_type: str, source_path: str) -> int:
        if self._in_stub_mode:
            logger.debug(f"[DX12Backend] Stub shader for {source_path}")
            return 0x12345678

        entry = "VSMain" if shader_type == "vs" else "PSMain"
        profile = "vs_5_0" if shader_type == "vs" else "ps_5_0"

        if not os.path.exists(source_path):
            logger.warning(f"[DX12Backend] Shader file not found: {source_path}")
            return 0x12345678

        try:
            result = dx.compile_hlsl(source_path, entry, profile)
            logger.debug(f"[DX12Backend] Shader compiled ({shader_type}) – {hex(result)}")
            return result
        except Exception as e:
            logger.error(f"[DX12Backend] Shader compilation error: {e}")
            return 0x12345678

    def create_graphics_ps(self, vs_blob: int, ps_blob: int) -> int:
        if vs_blob == 0x12345678 or ps_blob == 0x12345678:
            logger.warning("[DX12Backend] Using stub shaders – returning stub PSO")
            return 0x87654321

        try:
            vs_ptr = ctypes.c_void_p(vs_blob)
            ps_ptr = ctypes.c_void_p(ps_blob)
            pso = dx.create_graphics_ps(self.device, vs_ptr, ps_ptr)

            if pso and hasattr(pso, "value") and pso.value:
                logger.debug(f"[DX12Backend] PSO created: {hex(pso.value)}")
                return pso.value
            else:
                logger.error("[DX12Backend] PSO creation failed")
                return 0x87654321
        except Exception as e:
            logger.error(f"[DX12Backend] PSO creation exception: {e}")
            return 0x87654321

    def set_graphics_pipeline(self, pso: Any) -> None:
        if not self._in_stub_mode and pso and pso != 0xFEEDC0DE:
            try:
                dx.set_graphics_pipeline(ctypes.c_void_p(pso))
            except Exception as e:
                logger.debug(f"[DX12Backend] Set pipeline failed: {e}")

    # -----------------------------------------------------------------
    #   Buffers
    # -----------------------------------------------------------------
    def create_buffer(self, data: bytes, usage: str = "default") -> Any:
        if self._in_stub_mode or not self.device or not self.device.value:
            return ctypes.c_void_p(0xDEADBEEF)

        buf = dx.create_buffer(self.device, len(data), usage)
        if not buf or not buf.value or buf.value == 0xDEADBEEF:
            return ctypes.c_void_p(0xDEADBEEF)

        self.update_buffer(buf, data)
        self._resources.append(buf)
        return buf

    def update_buffer(self, buffer: Any, data: bytes) -> None:
        if self._in_stub_mode:
            return
        try:
            dx.update_subresource(buffer, data)
        except Exception as e:
            logger.debug(f"[DX12Backend] Buffer update failed: {e}")

    def create_constant_buffer(self, data: bytes) -> Any:
        return self.create_buffer(data, usage="constant")

    # -----------------------------------------------------------------
    #   Textures
    # -----------------------------------------------------------------
    def create_texture(self,
        data: bytes | None,
        w: int,
        h: int,
        fmt: str = "RGBA8",
    ) -> DX12Texture:
        logger.debug(f"[DX12Backend] Creating texture {w}×{h} fmt={fmt}")

        if self._in_stub_mode or not self.device or not self.device.value:
            dummy = ctypes.c_void_p(0xDEADBEEF + w + h)
            tex = DX12Texture(dummy)
            tex._srv_gpu = 0xDEADDEAD
            return tex

        try:
            fmt_bytes = fmt.lower().encode("utf-8")
            tex_ptr = dx.create_texture_from_memory(
                self.device, None, w, h, fmt_bytes
            )
            if not tex_ptr or not tex_ptr.value:
                raise RuntimeError("Native texture creation returned nullptr")

            tex = DX12Texture(tex_ptr)

            if data is not None:
                self.update_texture(tex, data, w, h)

            if self.cbv_srv_uav_heap:
                idx = self.cbv_srv_uav_heap.next_free()
                cpu_handle = self.cbv_srv_uav_heap.get_cpu_handle(idx)
                self.create_shader_resource_view(tex, cpu_handle)
                tex._srv_gpu = self.cbv_srv_uav_heap.get_gpu_handle(idx)
            else:
                tex._srv_gpu = 0xDEADDEAD

            self._resources.append(tex.ptr)
            return tex
        except Exception as e:
            logger.error(f"[DX12Backend] Texture creation exception: {e}")
            dummy = ctypes.c_void_p(0xDEADBEEF + w + h)
            tex = DX12Texture(dummy)
            tex._srv_gpu = 0xDEADDEAD
            return tex

    # -----------------------------------------------------------------
    #   Descriptor heaps
    # -----------------------------------------------------------------
    def create_descriptor_heap(self,
        num_descriptors: int,
        heap_type: str = "cbv_srv_uav",
    ) -> Any:
        if heap_type == "rtv":
            return self.rtv_heap
        if heap_type == "cbv_srv_uav":
            return self.cbv_srv_uav_heap
        raise ValueError(f"Unsupported heap type: {heap_type}")

    def get_cpu_handle(self, heap: Any, index: int) -> int:
        return heap.get_cpu_handle(index)

    def get_gpu_handle(self, heap: Any, index: int) -> int:
        return heap.get_gpu_handle(index)

    # -----------------------------------------------------------------
    #   SRV / RTV creation
    # -----------------------------------------------------------------
    def create_shader_resource_view(self, resource: Any, cpu_handle) -> None:
        if self._in_stub_mode:
            return
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            return
        try:
            cpu = ctypes.c_void_p(int(cpu_handle))
            dx.create_shader_resource_view(self.device, ptr, cpu)
        except Exception as e:
            logger.debug(f"[DX12Backend] SRV creation failed: {e}")

    def create_render_target_view(self, resource: Any, cpu_handle) -> None:
        if self._in_stub_mode:
            return
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            return
        try:
            cpu = ctypes.c_void_p(int(cpu_handle))
            dx.create_render_target_view(self.device, ptr, cpu)
        except Exception as e:
            logger.debug(f"[DX12Backend] RTV creation failed: {e}")

    # -----------------------------------------------------------------
    #   Root‑descriptor‑table
    # -----------------------------------------------------------------
    def set_root_descriptor_table(self, root_index: int, gpu_handle: Any) -> None:
        if not self._in_stub_mode:
            try:
                dx.set_root_descriptor_table(root_index, gpu_handle)
            except Exception as e:
                logger.debug(f"[DX12Backend] Set root descriptor table failed: {e}")

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> None:
        if not self._in_stub_mode:
            try:
                dx.set_descriptor_heaps(tuple(heaps))
            except Exception as e:
                logger.debug(f"[DX12Backend] Set descriptor heaps failed: {e}")

    # -----------------------------------------------------------------
    #   Render‑target handling
    # -----------------------------------------------------------------
    def set_render_target(self, rtv: Any) -> None:
        if not self._in_stub_mode:
            try:
                dx.set_render_target(rtv)
            except Exception as e:
                logger.debug(f"[DX12Backend] Set render target failed: {e}")

    def set_render_targets(self, rtvs: Sequence[Any]) -> None:
        if not self._in_stub_mode:
            try:
                dx.set_render_targets(tuple(rtvs))
            except Exception as e:
                logger.debug(f"[DX12Backend] Set render targets failed: {e}")

    def clear_render_target(self,
        rtv: Any,
        color: Tuple[float, float, float, float] = (0.0, 0.0, 0.0, 1.0),
    ) -> None:
        if not self._in_stub_mode:
            try:
                dx.clear_render_target(rtv, color)
            except Exception as e:
                logger.debug(f"[DX12Backend] Clear render target failed: {e}")

    # -----------------------------------------------------------------
    #   Viewport / Scissor
    # -----------------------------------------------------------------
    def set_viewport(self,
        x: int, y: int, w: int, h: int,
        min_depth: float = 0.0,
        max_depth: float = 1.0,
    ) -> None:
        self.viewport = (x, y, w, h)
        if not self._in_stub_mode:
            try:
                dx.set_viewport(x, y, w, h, min_depth, max_depth)
            except Exception as e:
                logger.debug(f"[DX12Backend] Set viewport failed: {e}")

    def set_scissor_rect(self,
        left: int, top: int, right: int, bottom: int,
    ) -> None:
        self.scissor = (left, top, right, bottom)
        if not self._in_stub_mode:
            try:
                dx.set_scissor_rect(left, top, right, bottom)
            except Exception as e:
                logger.debug(f"[DX12Backend] Set scissor rect failed: {e}")

    # -----------------------------------------------------------------
    #   Vertex / Index buffers & draw calls
    # -----------------------------------------------------------------
    def set_vertex_buffers(self,
        vertex_buffer: Any,
        index_buffer: Optional[Any] = None,
    ) -> None:
        if not self._in_stub_mode:
            try:
                dx.set_vertex_buffers(vertex_buffer, index_buffer)
            except Exception as e:
                logger.debug(f"[DX12Backend] Set vertex buffers failed: {e}")

    def draw(self, vertex_count: int, start_vertex: int = 0,
             instance_count: int = 1) -> None:
        if not self._in_stub_mode:
            try:
                dx.draw_instanced(
                    vertex_count, instance_count, start_vertex, 0
                )
            except Exception as e:
                logger.debug(f"[DX12Backend] Draw failed: {e}")

    def draw_indexed(self,
        index_count: int,
        start_index: int = 0,
        base_vertex: int = 0,
        instance_count: int = 1,
    ) -> None:
        if not self._in_stub_mode:
            try:
                dx.draw_indexed_instanced(
                    index_count,
                    instance_count,
                    start_index,
                    base_vertex,
                    0,
                )
            except Exception as e:
                logger.debug(f"[DX12Backend] Draw indexed failed: {e}")

    def draw_fullscreen_quad(self,
        pso: Any,
        descriptor_heaps: Sequence[Any],
        root_parameters: Sequence[Tuple[int, Any]],
    ) -> None:
        if self._in_stub_mode:
            return
        try:
            self.set_graphics_pipeline(pso)
            self.set_descriptor_heaps(descriptor_heaps)
            for root_idx, gpu_handle in root_parameters:
                self.set_root_descriptor_table(root_idx, gpu_handle)
            dx.draw_instanced(3, 1, 0, 0)   # full‑screen triangle
        except Exception as e:
            logger.debug(f"[DX12Backend] Draw fullscreen quad failed: {e}")

    # -----------------------------------------------------------------
    #   Sync / Release
    # -----------------------------------------------------------------
    def wait_for_gpu(self) -> None:
        if not self._in_stub_mode:
            try:
                dx.wait_for_gpu()
            except Exception as e:
                logger.debug(f"[DX12Backend] Wait for GPU failed: {e}")

    def release_resource(self, resource: Any) -> None:
        if resource and not self._in_stub_mode:
            try:
                dx.release_resource(resource)
            except Exception as e:
                logger.debug(f"[DX12Backend] Release resource failed: {e}")

    # -----------------------------------------------------------------
    #   Frame management
    # -----------------------------------------------------------------
    def enable_depth_test(self, enable: bool) -> None:
        self._depth_test_enabled = enable
        logger.debug("[DX12Backend] Depth test %s (stub)",
                     "enabled" if enable else "disabled")

    def begin_frame(self) -> None:
        logger.debug("[DX12Backend] begin_frame")
        # Reset‑allocator/command‑list делается в Rust‑модуле
        pass

    def end_frame(self) -> None:
        logger.debug("[DX12Backend] end_frame – presenting")
        self.present()
        self.wait_for_gpu()

    def shutdown(self) -> None:
        """Освободить все нативные ресурсы."""
        logger.info("[DX12Backend] Releasing all native resources")
        for r in self._resources:
            try:
                self.release_resource(r)
            except Exception as exc:
                logger.debug(f"Failed to release resource {r}: {exc}")
        self._resources.clear()

    # -----------------------------------------------------------------
    #   Текстурные апдейты (используется в forward‑renderer)
    # -----------------------------------------------------------------
    def update_texture(self, tex: Any, data: bytes, w: int, h: int) -> None:
        if self._in_stub_mode:
            return
        try:
            ptr = getattr(tex, "ptr", tex)
            dx.update_texture(ptr, data, ctypes.c_uint(w), ctypes.c_uint(h))
        except Exception as e:
            logger.debug(f"[DX12Backend] Update texture failed: {e}")

    # -----------------------------------------------------------------
    #   Информационные методы
    # -----------------------------------------------------------------
    def get_frame_index(self) -> int:
        return dx.get_frame_index()

    def get_rtv_descriptor_size(self) -> int:
        return dx.get_rtv_descriptor_size()

    def get_dsv_descriptor_size(self) -> int:
        return dx.get_dsv_descriptor_size()

    def recreate_swapchain_rtv(self) -> None:
        """
        Нужно вызвать после того, как приложение заменило `self.rtv_heap`
        на новый объект. Пересоздаёт RTV‑дескрипторы для текущей swap‑chain.
        """
        self._create_swapchain_rtv()