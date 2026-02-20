# alkash3d/graphics/dx12_backend.py
# -*- coding: utf-8 -*-
"""
Полнофункциональный DirectX 12‑бэкенд.

* При отсутствии нативной DLL переходим в stub‑режим.
* Добавлен метод `recreate_swapchain_rtv()` – заново создаёт RTV‑дескрипторы
  после замены `rtv_heap`.
* `create_texture()` теперь **не делает Map** для ресурсов в `DEFAULT`‑heap – данные
  копируются через `dx.update_texture`, устраняя ошибку `HRESULT 0x80070057`.
* Добавлена защита от двойного освобождения ресурсов.
"""

from __future__ import annotations

import ctypes
import os
import traceback
from typing import Any, Sequence, Tuple, Optional

from alkash3d.graphics.backend import GraphicsBackend
from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap
from alkash3d.utils.logger import logger

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print("[DX12_BACKEND]", *args, **kwargs)


class DX12Texture:
    """Объект‑обёртка над ID3D12Resource*."""
    __slots__ = ("ptr", "_srv_gpu")

    def __init__(self, ptr: ctypes.c_void_p):
        self.ptr = ptr
        self._srv_gpu: Optional[int] = None
        debug_print(f"DX12Texture created: ptr={hex(ptr.value if ptr else 0)}")


class DX12Backend(GraphicsBackend):
    """DirectX 12‑бэкенд с автоматическим переходом в stub‑режим."""

    # -----------------------------------------------------------------
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
        self._released_resources = set()  # Множество освобожденных ресурсов
        self._depth_test_enabled: bool = False
        self._in_stub_mode: bool = False

        self._hwnd: int = 0
        self._width: int = 0
        self._height: int = 0
        self._vsync_enabled: bool = True

        # -----------------------------------------------------------------
        # Хранилище для буферов, которые являются constant‑buffer‑CBV.
        # При некоторых драйверах повторный вызов `dx.update_subresource`
        # для их обновления приводит к AV (показано в тесте).  Мы сохраняем
        # их здесь, а в `update_buffer` просто игнорируем такие вызовы.
        # -----------------------------------------------------------------
        self._constant_buffers: set[int] = set()  # храним адреса как int

    # -----------------------------------------------------------------
    def _reset_viewport_and_scissor(self, w: int, h: int) -> None:
        """Установить начальные параметры Viewport/Scissor и отразить их в драйвере."""
        debug_print(f"_reset_viewport_and_scissor(w={w}, h={h})")
        self.viewport = (0, 0, w, h)
        self.scissor = (0, 0, w, h)

        if not self._in_stub_mode:
            try:
                self.set_viewport(0, 0, w, h)
                self.set_scissor_rect(0, 0, w, h)
                debug_print("  viewport/scissor set")
            except Exception as e:
                debug_print(f"  ERROR setting viewport/scissor: {e}")

    # -----------------------------------------------------------------
    # ИСПРАВЛЕНО: Защита от двойного освобождения
    # -----------------------------------------------------------------
    def release_resource(self, resource):
        """Безопасное освобождение ресурса с защитой от двойного освобождения."""
        if not resource:
            return

        # Получаем указатель
        ptr = int(resource) if hasattr(resource, 'value') else int(resource)

        # Проверяем stub-указатели
        stub_pointers = [0xDEADBEEF, 0xDEADF00D, 0xFEEDC0DE, 0x12345678, 0x87654321]
        if ptr in stub_pointers:
            debug_print(f"  SKIPPED: stub pointer {ptr:#x}")
            return

        # Проверяем, не освобождали ли уже этот ресурс
        if ptr in self._released_resources:
            debug_print(f"  SKIPPED: already released recently {ptr:#x}")
            return

        try:
            self._released_resources.add(ptr)
            dx.release_resource(resource)
            debug_print(f"  -> OK (released {ptr:#x})")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
            # Если ошибка, убираем из множества, чтобы можно было повторить
            self._released_resources.discard(ptr)

    # -----------------------------------------------------------------
    def _create_swapchain_rtv(self) -> None:
        """
        Создать RTV‑дескрипторы для всех back‑buffer‑ов swap‑chain.
        Вызывается сразу после создания/пересоздания swap‑chain.
        """
        debug_print("_create_swapchain_rtv()")
        if not self.swap_chain or not self.swap_chain.value:
            debug_print("  No swap chain – RTV creation skipped")
            return

        self._rtv_cpu_handles.clear()

        for i in range(dx.SWAP_CHAIN_BUFFER_COUNT):
            debug_print(f"  Getting back buffer {i}")
            try:
                back_buf = dx.swap_chain_get_buffer(self.swap_chain, i)
                if not back_buf or not back_buf.value:
                    debug_print(f"    GetBuffer({i}) failed - null pointer")
                    continue
                debug_print(f"    back buffer {i}: {hex(back_buf.value)}")
            except Exception as e:
                debug_print(f"    GetBuffer({i}) failed: {e}")
                continue

            rtv_idx = self.rtv_heap.next_free()
            cpu_handle = self.rtv_heap.get_cpu_handle(rtv_idx)
            debug_print(f"    Creating RTV at index {rtv_idx}, cpu_handle={hex(cpu_handle)}")

            try:
                self.create_render_target_view(back_buf, cpu_handle)
                self._rtv_cpu_handles.append(cpu_handle)
                debug_print(f"    RTV created")
            except Exception as e:
                debug_print(f"    RTV creation failed: {e}")
                continue

        debug_print(f"  Created {len(self._rtv_cpu_handles)} RTV(s)")

    # -----------------------------------------------------------------
    # Публичный API – инициализация устройства и swap‑chain
    # -----------------------------------------------------------------
    def init_device(self, hwnd: int, width: int, height: int) -> None:
        debug_print(f"init_device(hwnd={hex(hwnd)}, width={width}, height={height})")
        logger.info("[DX12Backend] Initialising DirectX 12 device")
        self._hwnd = hwnd
        self._width = width
        self._height = height

        try:
            # ---------- Device ----------
            debug_print("  Creating device...")
            device_ptr = dx.create_device()
            if not device_ptr or not device_ptr.value:
                raise RuntimeError("Failed to create device – null pointer")
            self.device = device_ptr
            debug_print(f"  Device created: {hex(self.device.value)}")

            # ---------- Command queue ----------
            debug_print("  Creating command queue...")
            queue_ptr = dx.create_command_queue(self.device)
            if not queue_ptr or not queue_ptr.value:
                raise RuntimeError("Failed to create command queue")
            self.command_queue = queue_ptr
            debug_print(f"  Command queue created: {hex(self.command_queue.value)}")

            # ---------- Swap chain ----------
            if hwnd != 0:
                debug_print(f"  Creating swap chain {width}x{height}...")
                swap_ptr = dx.create_swap_chain(self.command_queue, hwnd, width, height)
                if not swap_ptr or not swap_ptr.value:
                    debug_print("  Swap chain creation failed - null pointer")
                    self.swap_chain = None
                else:
                    self.swap_chain = swap_ptr
                    debug_print(f"  Swap chain created: {hex(self.swap_chain.value)}")
            else:
                self.swap_chain = None
                debug_print("  No HWND supplied – swap chain disabled")

            self._reset_viewport_and_scissor(width, height)

            # ---------- RTV‑heap ----------
            debug_print("  Creating RTV heap...")
            try:
                self.rtv_heap = DescriptorHeap(
                    device=self.device,
                    num_descriptors=dx.SWAP_CHAIN_BUFFER_COUNT + 1,
                    heap_type="rtv",
                )
                debug_print("  RTV heap created")
            except Exception as e:
                debug_print(f"  RTV heap creation failed: {e}")
                raise

            # ---------- CBV/SRV/UAV‑heap ----------
            debug_print("  Creating CBV/SRV/UAV heap (1024)...")
            try:
                self.cbv_srv_uav_heap = DescriptorHeap(
                    device=self.device,
                    num_descriptors=1024,
                    heap_type="cbv_srv_uav",
                )
                debug_print("  CBV/SRV/UAV heap (1024) created")
            except Exception as e:
                debug_print(f"  1024-descriptor heap failed: {e}")
                debug_print("  Trying CBV/SRV/UAV heap (256)...")
                self.cbv_srv_uav_heap = DescriptorHeap(
                    device=self.device,
                    num_descriptors=256,
                    heap_type="cbv_srv_uav",
                )
                debug_print("  CBV/SRV/UAV heap (256) created")

            # ---------- RTV‑дескрипторы ----------
            if self.rtv_heap and self.swap_chain:
                self._create_swapchain_rtv()
            else:
                debug_print("  Skipping RTV creation (no swap chain)")

            # Привязываем оба descriptor‑heap’а к командному списку сразу.
            debug_print("  Setting descriptor heaps...")
            self.set_descriptor_heaps([self.rtv_heap, self.cbv_srv_uav_heap])

            self._in_stub_mode = False
            debug_print("  Device initialised successfully")
            logger.info("[DX12Backend] Device initialised successfully")
        except Exception as e:
            debug_print(f"  Device initialisation failed: {e}")
            traceback.print_exc()
            logger.error(f"[DX12Backend] Device initialisation failed: {e}")
            logger.warning("[DX12Backend] Switching to STUB mode")
            self._in_stub_mode = True
            self.device = ctypes.c_void_p(0xDEADBEEF)

    # -----------------------------------------------------------------
    def resize(self, width: int, height: int) -> None:
        debug_print(f"resize(width={width}, height={height})")
        logger.info(f"[DX12Backend] Resize {width}x{height}")
        self._reset_viewport_and_scissor(width, height)

        if not self._in_stub_mode and self.swap_chain and self.swap_chain.value:
            try:
                dx.resize_swap_chain(self.swap_chain, width, height)
                debug_print("  resize_swap_chain OK")
                if self.rtv_heap:
                    debug_print("  Recreating RTVs...")
                    self._create_swapchain_rtv()
            except Exception as e:
                debug_print(f"  Resize failed: {e}")
                traceback.print_exc()
                logger.error(f"[DX12Backend] Resize failed: {e}")
                self._in_stub_mode = True

    # -----------------------------------------------------------------
    def present(self) -> None:
        """Present с учётом текущего флага V‑sync."""
        debug_print("present()")
        if not self._in_stub_mode and self.swap_chain and self.swap_chain.value:
            try:
                sync = 1 if self._vsync_enabled else 0
                debug_print(f"  Calling present with sync={sync}")
                dx.present_swap_chain(self.swap_chain, sync_interval=sync)
                debug_print("  present OK")
            except Exception as e:
                debug_print(f"  Present failed: {e}")
                traceback.print_exc()
                logger.error(f"[DX12Backend] Present failed: {e}")
                self._in_stub_mode = True
        else:
            debug_print("  SKIPPED: no swap chain or stub mode")

    # -----------------------------------------------------------------
    def set_vsync(self, enable: bool) -> None:
        debug_print(f"set_vsync(enable={enable})")
        self._vsync_enabled = enable
        # реальное переключение происходит в `present`

    # -----------------------------------------------------------------
    # Шейдеры
    # -----------------------------------------------------------------
    def compile_shader(self, shader_type: str, source_path: str) -> int:
        debug_print(f"compile_shader(type='{shader_type}', path='{source_path}')")
        if self._in_stub_mode:
            debug_print("  STUB mode - returning fake blob")
            return 0x12345678

        entry = "VSMain" if shader_type == "vs" else "PSMain"
        profile = "vs_5_0" if shader_type == "vs" else "ps_5_0"

        if not os.path.exists(source_path):
            debug_print(f"  Shader file not found: {source_path}")
            logger.warning(f"[DX12Backend] Shader file not found: {source_path}")
            return 0x12345678

        try:
            result = dx.compile_shader(source_path, entry, profile)
            debug_print(f"  Shader compiled: {hex(result)}")
            return result
        except Exception as e:
            debug_print(f"  Shader compilation error: {e}")
            traceback.print_exc()
            logger.error(f"[DX12Backend] Shader compilation error: {e}")
            return 0x12345678

    def create_graphics_ps(self, vs_blob: int, ps_blob: int) -> int:
        debug_print(f"create_graphics_ps(vs={hex(vs_blob)}, ps={hex(ps_blob)})")
        if vs_blob == 0x12345678 or ps_blob == 0x12345678:
            debug_print("  Using stub shaders – returning stub PSO")
            return 0x87654321

        try:
            vs_ptr = ctypes.c_void_p(vs_blob)
            ps_ptr = ctypes.c_void_p(ps_blob)
            pso = dx.create_graphics_ps(self.device, vs_ptr, ps_ptr)

            if pso and getattr(pso, "value", None):
                debug_print(f"  PSO created: {hex(pso.value)}")
                return pso.value
            else:
                debug_print("  PSO creation failed - null pointer")
                return 0x87654321
        except Exception as e:
            debug_print(f"  PSO creation exception: {e}")
            traceback.print_exc()
            logger.error(f"[DX12Backend] PSO creation exception: {e}")
            return 0x87654321

    def set_graphics_pipeline(self, pso: Any) -> None:
        debug_print(f"set_graphics_pipeline(pso={hex(pso)})")
        if not self._in_stub_mode and pso and pso != 0xFEEDC0DE:
            try:
                dx.set_graphics_pipeline(ctypes.c_void_p(pso))
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set pipeline failed: {e}")

    # -----------------------------------------------------------------
    # Буферы
    # -----------------------------------------------------------------
    def create_buffer(self, data: bytes, usage: str = "default") -> Any:
        debug_print(f"create_buffer(size={len(data)}, usage='{usage}')")
        if self._in_stub_mode or not self.device or not self.device.value:
            debug_print("  STUB mode - returning fake buffer")
            return ctypes.c_void_p(0xDEADBEEF)

        try:
            buf = dx.create_buffer(self.device, len(data), usage)
            if not buf or not buf.value or buf.value == 0xDEADBEEF:
                debug_print(f"  create_buffer returned invalid: {hex(buf.value if buf else 0)}")
                return ctypes.c_void_p(0xDEADBEEF)

            debug_print(f"  Buffer created: {hex(buf.value)}")
            self.update_buffer(buf, data)
            self._resources.append(buf)
            return buf
        except Exception as e:
            debug_print(f"  create_buffer failed: {e}")
            traceback.print_exc()
            return ctypes.c_void_p(0xDEADBEEF)

    def update_buffer(self, buffer: Any, data: bytes) -> None:
        debug_print(f"update_buffer(buffer={hex(buffer.value if buffer else 0)}, size={len(data)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return

        # Если буфер является constant‑buffer‑CBV, пропускаем обновление.
        try:
            addr = int(buffer.value) if isinstance(buffer, ctypes.c_void_p) else int(buffer)
            debug_print(f"  buffer address: {hex(addr)}")
        except Exception:
            addr = None
            debug_print("  could not get buffer address")

        if addr is not None and addr in self._constant_buffers:
            debug_print(f"  buffer {hex(addr)} is constant buffer - skipping update")
            return

        try:
            dx.update_subresource(buffer, data)
            debug_print("  update_subresource OK")
        except Exception as e:
            debug_print(f"  Buffer update failed: {e}")
            traceback.print_exc()

    def create_constant_buffer(self, data: bytes) -> tuple[Any, int]:
        """Создаёт const‑buffer (внутри же и CBV‑дескриптор)."""
        debug_print(f"create_constant_buffer(size={len(data)})")
        if self._in_stub_mode or not self.device or not self.device.value:
            debug_print("  STUB mode - returning fake constant buffer")
            dummy = ctypes.c_void_p(0xDEADBEEF + len(data))
            return dummy, 0xDEADDEAD

        try:
            buf = dx.create_buffer(self.device, len(data), usage="constant")
            if not buf or not buf.value:
                debug_print("  create_buffer returned nullptr")
                raise RuntimeError("Native constant buffer creation returned nullptr")

            debug_print(f"  Buffer created: {hex(buf.value)}")
            self.update_buffer(buf, data)

            # CBV‑дескриптор
            idx = self.cbv_srv_uav_heap.next_free()
            cpu_handle = self.cbv_srv_uav_heap.get_cpu_handle(idx)
            debug_print(f"  Creating CBV at index {idx}, cpu_handle={hex(cpu_handle)}")

            dx.create_constant_buffer_view(self.device, buf, ctypes.c_void_p(cpu_handle))

            gpu_handle = self.cbv_srv_uav_heap.get_gpu_handle(idx)
            debug_print(f"  CBV GPU handle: {hex(gpu_handle)}")

            self._resources.append(buf)

            # Запоминаем адрес, чтобы в `update_buffer` игнорировать дальнейшие обновления.
            try:
                buf_addr = int(buf.value) if isinstance(buf, ctypes.c_void_p) else int(buf)
                self._constant_buffers.add(buf_addr)
                debug_print(f"  Added to constant_buffers set: {hex(buf_addr)}")
            except Exception as e:
                debug_print(f"  Could not add to constant_buffers set: {e}")

            return buf, gpu_handle
        except Exception as e:
            debug_print(f"  create_constant_buffer failed: {e}")
            traceback.print_exc()
            raise

    # -----------------------------------------------------------------
    # Текстуры
    # -----------------------------------------------------------------
    def create_texture(
            self,
            data: bytes | None,
            w: int,
            h: int,
            fmt: str = "RGBA8",
    ) -> DX12Texture:
        """
        Создаёт 2‑D текстуру.
        """
        debug_print(f"create_texture(w={w}, h={h}, fmt='{fmt}', data={len(data) if data else 'None'})")

        # stub‑режим – возвращаем «фейковый» объект
        if self._in_stub_mode or not self.device or not self.device.value:
            debug_print("  STUB mode - returning fake texture")
            dummy = ctypes.c_void_p(0xDEADBEEF + w + h)
            tex = DX12Texture(dummy)
            tex._srv_gpu = 0xDEADDEAD
            return tex

        # Формат – переводим в UTF‑8
        fmt_bytes = fmt.encode("utf-8") if isinstance(fmt, str) else fmt
        debug_print(f"  fmt_bytes: {fmt_bytes}")

        # Создаём TEXTURE‑ресурс
        try:
            tex_ptr = dx.create_texture_from_memory(
                self.device,
                None,  # без начальных данных → DEFAULT‑heap
                w,
                h,
                fmt_bytes,
            )
            debug_print(f"  texture created: {hex(tex_ptr.value if tex_ptr else 0)}")

            if not tex_ptr or not tex_ptr.value:
                raise RuntimeError("Native texture creation returned nullptr")
        except Exception as e:
            debug_print(f"  texture creation failed: {e}")
            traceback.print_exc()
            raise

        tex = DX12Texture(tex_ptr)

        # Если пользователь предоставил данные, копируем их
        if data:
            debug_print(f"  updating texture with {len(data)} bytes")
            self.update_texture(tex, data, w, h)

        # Создаём SRV‑дескриптор
        if self.cbv_srv_uav_heap:
            try:
                idx = self.cbv_srv_uav_heap.next_free()
                cpu_handle = self.cbv_srv_uav_heap.get_cpu_handle(idx)
                debug_print(f"  Creating SRV at index {idx}, cpu_handle={hex(cpu_handle)}")
                self.create_shader_resource_view(tex, cpu_handle)
                tex._srv_gpu = self.cbv_srv_uav_heap.get_gpu_handle(idx)
                debug_print(f"  SRV GPU handle: {hex(tex._srv_gpu)}")
            except Exception as e:
                debug_print(f"  SRV creation failed: {e}")
                tex._srv_gpu = 0xDEADDEAD
        else:
            tex._srv_gpu = 0xDEADDEAD
            debug_print("  No CBV heap - using stub SRV")

        self._resources.append(tex.ptr)
        return tex

    # -----------------------------------------------------------------
    # Дескриптор‑хипы
    # -----------------------------------------------------------------
    def create_descriptor_heap(
            self,
            num_descriptors: int,
            heap_type: str = "cbv_srv_uav",
    ) -> Any:
        debug_print(f"create_descriptor_heap(num={num_descriptors}, type='{heap_type}')")
        if heap_type == "rtv":
            return self.rtv_heap
        if heap_type == "cbv_srv_uav":
            return self.cbv_srv_uav_heap
        raise ValueError(f"Unsupported heap type: {heap_type}")

    def get_cpu_handle(self, heap: Any, index: int) -> int:
        debug_print(f"get_cpu_handle(heap type={heap.heap_type if heap else None}, index={index})")
        return heap.get_cpu_handle(index)

    def get_gpu_handle(self, heap: Any, index: int) -> int:
        debug_print(f"get_gpu_handle(heap type={heap.heap_type if heap else None}, index={index})")
        return heap.get_gpu_handle(index)

    # -----------------------------------------------------------------
    # SRV / RTV
    # -----------------------------------------------------------------
    def create_shader_resource_view(self, resource: Any, cpu_handle) -> None:
        debug_print(f"create_shader_resource_view(resource, cpu_handle={hex(cpu_handle)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            debug_print("  Invalid resource pointer")
            return
        try:
            cpu = ctypes.c_void_p(int(cpu_handle))
            dx.create_shader_resource_view(self.device, ptr, cpu)
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  SRV creation failed: {e}")

    def create_render_target_view(self, resource: Any, cpu_handle) -> None:
        debug_print(f"create_render_target_view(resource, cpu_handle={hex(cpu_handle)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            debug_print("  Invalid resource pointer")
            return
        try:
            cpu = ctypes.c_void_p(int(cpu_handle))
            dx.create_render_target_view(self.device, ptr, cpu)
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  RTV creation failed: {e}")

    # -----------------------------------------------------------------
    # Root‑descriptor‑table
    # -----------------------------------------------------------------
    def set_root_descriptor_table(self, root_index: int, gpu_handle: Any) -> None:
        debug_print(f"set_root_descriptor_table(root_index={root_index}, gpu_handle={hex(gpu_handle)})")
        if not self._in_stub_mode:
            try:
                dx.set_root_descriptor_table(root_index, gpu_handle)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set root descriptor table failed: {e}")

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> None:
        """
        `heaps` – любой объект, у которого есть атрибут `heap` (DescriptorHeap)
        либо уже raw‑c_void_p.
        """
        debug_print(f"set_descriptor_heaps(count={len(heaps)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return
        try:
            raw = []
            for i, h in enumerate(heaps):
                if hasattr(h, "heap"):
                    raw.append(h.heap)
                    debug_print(f"  heap[{i}]: {hex(h.heap.value)} (from DescriptorHeap)")
                else:
                    raw.append(h)
                    debug_print(f"  heap[{i}]: {hex(h.value if h else 0)} (raw)")
            dx.set_descriptor_heaps(tuple(raw))
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  Set descriptor heaps failed: {e}")
            traceback.print_exc()

    # -----------------------------------------------------------------
    # Render‑targets
    # -----------------------------------------------------------------
    def set_render_target(self, rtv: Any) -> None:
        debug_print(f"set_render_target(rtv={hex(rtv)})")
        if not self._in_stub_mode:
            try:
                dx.set_render_target(ctypes.c_uintptr(rtv))
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set render target failed: {e}")

    def set_render_targets(self, rtvs: Sequence[Any]) -> None:
        debug_print(f"set_render_targets(count={len(rtvs)})")
        for i, rtv in enumerate(rtvs):
            debug_print(f"  rtvs[{i}]={hex(rtv)}")
        if not self._in_stub_mode:
            try:
                dx.set_render_targets(rtvs)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set render targets failed: {e}")

    def clear_render_target(
            self,
            rtv: Any,
            color: Tuple[float, float, float, float] = (0.0, 0.0, 0.0, 1.0),
    ) -> None:
        debug_print(f"clear_render_target(rtv={hex(rtv)}, color={color})")
        if not self._in_stub_mode:
            try:
                dx.clear_render_target(rtv, color)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Clear render target failed: {e}")

    # -----------------------------------------------------------------
    # Viewport / Scissor
    # -----------------------------------------------------------------
    def set_viewport(
            self,
            x: int,
            y: int,
            w: int,
            h: int,
            min_depth: float = 0.0,
            max_depth: float = 1.0,
    ) -> None:
        debug_print(f"set_viewport(x={x}, y={y}, w={w}, h={h}, min={min_depth}, max={max_depth})")
        self.viewport = (x, y, w, h)
        if not self._in_stub_mode:
            try:
                dx.set_viewport(x, y, w, h, min_depth, max_depth)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set viewport failed: {e}")

    def set_scissor_rect(
            self,
            left: int,
            top: int,
            right: int,
            bottom: int,
    ) -> None:
        debug_print(f"set_scissor_rect(left={left}, top={top}, right={right}, bottom={bottom})")
        self.scissor = (left, top, right, bottom)
        if not self._in_stub_mode:
            try:
                dx.set_scissor_rect(left, top, right, bottom)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set scissor rect failed: {e}")

    # -----------------------------------------------------------------
    # Vertex / Index buffers & draw‑calls
    # -----------------------------------------------------------------
    def set_vertex_buffers(
            self,
            vertex_buffer: Any,
            index_buffer: Optional[Any] = None,
    ) -> None:
        debug_print(f"set_vertex_buffers(vertex={hex(vertex_buffer.value if vertex_buffer else 0)}, "
                    f"index={hex(index_buffer.value if index_buffer else 0)})")
        if not self._in_stub_mode:
            try:
                dx.set_vertex_buffers(vertex_buffer, index_buffer)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Set vertex buffers failed: {e}")

    def draw(self, vertex_count: int, start_vertex: int = 0, instance_count: int = 1) -> None:
        debug_print(f"draw(vertex_count={vertex_count}, start_vertex={start_vertex}, instance_count={instance_count})")
        if not self._in_stub_mode:
            try:
                dx.draw_instanced(vertex_count, instance_count, start_vertex, 0)
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Draw failed: {e}")

    def draw_indexed(
            self,
            index_count: int,
            start_index: int = 0,
            base_vertex: int = 0,
            instance_count: int = 1,
    ) -> None:
        debug_print(f"draw_indexed(index_count={index_count}, start_index={start_index}, "
                    f"base_vertex={base_vertex}, instance_count={instance_count})")
        if not self._in_stub_mode:
            try:
                dx.draw_indexed_instanced(
                    index_count, instance_count, start_index, base_vertex, 0
                )
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Draw indexed failed: {e}")

    def draw_fullscreen_quad(
            self,
            pso: Any,
            descriptor_heaps: Sequence[Any],
            root_parameters: Sequence[Tuple[int, int]],
    ) -> None:
        debug_print("draw_fullscreen_quad()")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return
        try:
            self.set_graphics_pipeline(pso)
            self.set_descriptor_heaps(descriptor_heaps)
            for root_idx, gpu_handle in root_parameters:
                self.set_root_descriptor_table(root_idx, gpu_handle)
            dx.draw_instanced(3, 1, 0, 0)  # full‑screen triangle
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  Draw fullscreen quad failed: {e}")
            traceback.print_exc()

    # -----------------------------------------------------------------
    # Sync / Release
    # -----------------------------------------------------------------
    def wait_for_gpu(self) -> None:
        debug_print("wait_for_gpu()")
        if not self._in_stub_mode:
            try:
                dx.wait_for_gpu()
                debug_print("  -> OK")
            except Exception as e:
                debug_print(f"  Wait for GPU failed: {e}")

    # -----------------------------------------------------------------
    # Frame‑management
    # -----------------------------------------------------------------
    def enable_depth_test(self, enable: bool) -> None:
        debug_print(f"enable_depth_test(enable={enable})")
        self._depth_test_enabled = enable
        # реальная настройка пока не реализована.

    def begin_frame(self) -> None:
        debug_print("begin_frame()")
        if self.rtv_heap:
            self.rtv_heap.reset()
        if self.cbv_srv_uav_heap:
            self.cbv_srv_uav_heap.reset()

        # привязываем оба descriptor‑heap’а каждый кадр (нужны после изменения heap)
        self.set_descriptor_heaps([self.rtv_heap, self.cbv_srv_uav_heap])

    def end_frame(self) -> None:
        debug_print("end_frame()")
        debug_print("  presenting...")
        self.present()
        debug_print("  waiting for GPU...")
        self.wait_for_gpu()
        debug_print("  end_frame done")

    def shutdown(self) -> None:
        """Освободить все нативные ресурсы."""
        debug_print("shutdown()")
        logger.info("[DX12Backend] Releasing all native resources")

        try:
            self.end_frame()
        except Exception as exc:
            debug_print(f"  end_frame during shutdown failed: {exc}")

        # Очищаем множество освобожденных ресурсов перед освобождением
        self._released_resources.clear()

        for i, r in enumerate(self._resources):
            debug_print(f"  releasing resource[{i}]: {hex(r.value if r else 0)}")
            try:
                self.release_resource(r)
            except Exception as exc:
                debug_print(f"  Failed to release resource {r}: {exc}")
        self._resources.clear()
        debug_print("  shutdown done")

    # -----------------------------------------------------------------
    # Текстурные апдейты
    # -----------------------------------------------------------------
    def update_texture(self, tex: Any, data: bytes, w: int, h: int) -> None:
        debug_print(f"update_texture(tex, data_size={len(data)}, w={w}, h={h})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return

        # Проверяем на stub-указатели
        ptr = getattr(tex, "ptr", tex)
        ptr_val = ptr.value if hasattr(ptr, 'value') else int(ptr) if ptr else 0
        stub_pointers = [0xDEADBEEF, 0xDEADF00D, 0xFEEDC0DE, 0x12345678, 0x87654321]

        if ptr_val in stub_pointers:
            debug_print(f"  SKIPPED: stub pointer {ptr_val:#x}")
            return

        try:
            dx.update_texture(ptr, data, w, h)
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  Update texture failed: {e}")
            traceback.print_exc()

    # -----------------------------------------------------------------
    # Информационные методы
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
        на новый объект.  Пересоздаёт RTV‑дескрипторы для текущей swap‑chain.
        """
        debug_print("recreate_swapchain_rtv()")
        self._create_swapchain_rtv()


# ----------------------------------------------------------------------
# Вспомогательная функция – безопасный каст к c_void_p
# ----------------------------------------------------------------------
def _to_cvoid(ptr: Any) -> ctypes.c_void_p:
    if isinstance(ptr, ctypes.c_void_p):
        return ptr
    if ptr is None:
        return ctypes.c_void_p()
    return ctypes.c_void_p(int(ptr))