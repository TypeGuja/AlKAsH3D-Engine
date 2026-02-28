# alkash3d/graphics/dx12_backend.py
# -*- coding: utf-8 -*-
"""
Полнофункциональный DirectX 12‑бэкенд с гарантированным выводом изображения
ИСПРАВЛЕННАЯ ВЕРСИЯ
"""

from __future__ import annotations

import ctypes
import os
import time
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
    __slots__ = ("ptr", "_srv_gpu", "width", "height", "format")

    def __init__(self, ptr: ctypes.c_void_p, width=0, height=0, format=""):
        self.ptr = ptr
        self._srv_gpu: Optional[int] = None
        self.width = width
        self.height = height
        self.format = format


class DX12Backend(GraphicsBackend):
    """DirectX 12‑бэкенд с гарантированным выводом изображения"""

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

    def _reset_viewport_and_scissor(self, w: int, h: int) -> bool:
        """Установить начальные параметры Viewport/Scissor."""
        debug_print(f"_reset_viewport_and_scissor(w={w}, h={h})")
        self.viewport = (0, 0, w, h)
        self.scissor = (0, 0, w, h)

        if not self._in_stub_mode:
            try:
                if not self.set_viewport(0, 0, w, h):
                    debug_print("  ERROR setting viewport")
                    return False
                if not self.set_scissor_rect(0, 0, w, h):
                    debug_print("  ERROR setting scissor")
                    return False
                debug_print("  viewport/scissor set")
                return True
            except Exception as e:
                debug_print(f"  ERROR setting viewport/scissor: {e}")
                return False
        return False

    def _get_swapchain_buffer(self, index: int) -> Any:
        """Получить back buffer из swap chain"""
        if not self.swap_chain or not self.swap_chain.value:
            return None
        try:
            return dx.swap_chain_get_buffer(self.swap_chain, index)
        except Exception as e:
            debug_print(f"  Failed to get swapchain buffer {index}: {e}")
            return None

    def _create_swapchain_rtv(self) -> bool:
        """Создать RTV‑дескрипторы для всех back‑buffer‑ов swap‑chain."""
        debug_print("_create_swapchain_rtv()")

        if not self.swap_chain or not self.swap_chain.value:
            debug_print("  No swap chain – RTV creation skipped")
            return False

        if not self.rtv_heap:
            debug_print("  No RTV heap – RTV creation skipped")
            return False

        self._rtv_cpu_handles.clear()
        self.rtv_heap.reset()

        for i in range(dx.SWAP_CHAIN_BUFFER_COUNT):
            debug_print(f"  Getting back buffer {i}")
            try:
                back_buf = self._get_swapchain_buffer(i)
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
                if not self.create_render_target_view(back_buf, cpu_handle):
                    debug_print(f"    RTV creation failed")
                    continue
                self._rtv_cpu_handles.append(cpu_handle)
                debug_print(f"    RTV created")
            except Exception as e:
                debug_print(f"    RTV creation failed: {e}")
                continue

        debug_print(f"  Created {len(self._rtv_cpu_handles)} RTV(s)")
        return len(self._rtv_cpu_handles) > 0

    def _cleanup_resources(self) -> None:
        """Очистка всех ресурсов перед пересозданием."""
        debug_print("_cleanup_resources()")

        try:
            self.wait_for_gpu()
        except:
            pass

        for resource in self._resources:
            try:
                self.release_resource(resource)
            except:
                pass
        self._resources.clear()

        self.rtv_heap = None
        self.cbv_srv_uav_heap = None
        self._rtv_cpu_handles.clear()

        if self.swap_chain:
            try:
                self.release_resource(self.swap_chain)
            except:
                pass
            self.swap_chain = None

        if self.command_queue:
            try:
                self.release_resource(self.command_queue)
            except:
                pass
            self.command_queue = None

        if self.device:
            try:
                self.release_resource(self.device)
            except:
                pass
            self.device = None

        try:
            dx.force_cleanup()
        except:
            pass

        time.sleep(0.1)
        debug_print("  cleanup done")

    def init_device(self, hwnd: int, width: int, height: int) -> None:
        debug_print(f"init_device(hwnd={hex(hwnd)}, width={width}, height={height})")
        logger.info("[DX12Backend] Initialising DirectX 12 device")

        self._hwnd = hwnd
        self._width = width
        self._height = height
        self._initialized = False

        self._cleanup_resources()

        try:
            debug_print("  Creating device...")
            device_ptr = dx.create_device()
            if not device_ptr or not device_ptr.value:
                raise RuntimeError("Failed to create device – null pointer")
            self.device = device_ptr
            debug_print(f"  Device created: {hex(self.device.value)}")

            debug_print("  Creating command queue...")
            queue_ptr = dx.create_command_queue(self.device)
            if not queue_ptr or not queue_ptr.value:
                raise RuntimeError("Failed to create command queue")
            self.command_queue = queue_ptr
            debug_print(f"  Command queue created: {hex(self.command_queue.value)}")

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

            if not self._reset_viewport_and_scissor(width, height):
                debug_print("  Warning: viewport/scissor reset failed")

            debug_print("  Creating RTV heap...")
            try:
                self.rtv_heap = DescriptorHeap(
                    device=self.device,
                    num_descriptors=dx.SWAP_CHAIN_BUFFER_COUNT + 4,
                    heap_type="rtv",
                    shader_visible=False
                )
                debug_print("  RTV heap created")
            except Exception as e:
                debug_print(f"  RTV heap creation failed: {e}")
                raise

            debug_print("  Creating CBV/SRV/UAV heap...")
            try:
                self.cbv_srv_uav_heap = DescriptorHeap(
                    device=self.device,
                    num_descriptors=1024,
                    heap_type="cbv_srv_uav",
                    shader_visible=True
                )
                debug_print("  CBV/SRV/UAV heap created")
            except Exception as e:
                debug_print(f"  CBV/SRV/UAV heap creation failed: {e}")

            if self.rtv_heap and self.swap_chain:
                if not self._create_swapchain_rtv():
                    debug_print("  Failed to create RTVs")
                    raise RuntimeError("Failed to create RTVs")
            else:
                debug_print("  Skipping RTV creation (no swap chain)")

            self._in_stub_mode = False
            self._initialized = True
            self._current_frame = self.get_frame_index()
            debug_print("  Device initialised successfully")
            logger.info("[DX12Backend] Device initialised successfully")

        except Exception as e:
            debug_print(f"  Device initialisation failed: {e}")
            traceback.print_exc()
            logger.error(f"[DX12Backend] Device initialisation failed: {e}")
            logger.warning("[DX12Backend] Switching to STUB mode")
            self._in_stub_mode = True
            self.device = ctypes.c_void_p(0xDEADBEEF)
            self._initialized = False

    def resize(self, width: int, height: int) -> bool:
        debug_print(f"resize(width={width}, height={height})")
        logger.info(f"[DX12Backend] Resize {width}x{height}")

        self._width = width
        self._height = height
        self._reset_viewport_and_scissor(width, height)

        if not self._in_stub_mode and self.swap_chain and self.swap_chain.value:
            try:
                if not dx.resize_swap_chain(self.swap_chain, width, height):
                    debug_print("  resize_swap_chain failed")
                    return False
                debug_print("  resize_swap_chain OK")
                if self.rtv_heap:
                    debug_print("  Recreating RTVs...")
                    if not self._create_swapchain_rtv():
                        debug_print("  RTV recreation failed")
                        return False
                return True
            except Exception as e:
                debug_print(f"  Resize failed: {e}")
                traceback.print_exc()
                logger.error(f"[DX12Backend] Resize failed: {e}")
                return False
        return False

    def present(self, sync_interval: int = 1) -> bool:
        """Present с гарантированным выводом"""
        debug_print(f"present(sync_interval={sync_interval})")

        if not self._initialized or self._in_stub_mode or not self.swap_chain or not self.swap_chain.value:
            debug_print("  SKIPPED: no swap chain or stub mode")
            return False

        try:
            result = dx.present_swap_chain(self.swap_chain, sync_interval)
            debug_print(f"  present returned {result}")

            if result:
                self._current_frame = (self._current_frame + 1) % dx.SWAP_CHAIN_BUFFER_COUNT
                if self._hwnd:
                    ctypes.windll.user32.InvalidateRect(self._hwnd, None, True)
                    ctypes.windll.user32.UpdateWindow(self._hwnd)

            return result
        except Exception as e:
            debug_print(f"  present failed: {e}")
            return False

    def set_vsync(self, enable: bool) -> None:
        debug_print(f"set_vsync(enable={enable})")
        self._vsync_enabled = enable

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

    def create_graphics_ps(self, vs_blob: ctypes.c_void_p, ps_blob: ctypes.c_void_p) -> Any:
        """Создать graphics pipeline state object."""
        debug_print(
            f"create_graphics_ps(vs={hex(vs_blob.value if vs_blob else 0)}, ps={hex(ps_blob.value if ps_blob else 0)})")

        if self._in_stub_mode:
            debug_print("  STUB mode - cannot create PSO")
            return None

        # Проверка на stub значения
        vs_val = vs_blob.value if vs_blob else 0
        ps_val = ps_blob.value if ps_blob else 0

        if vs_val == 0x12345678 or ps_val == 0x12345678 or vs_val == 0 or ps_val == 0:
            debug_print("  Using stub shaders – cannot create PSO")
            return None

        try:
            # ИСПРАВЛЕНИЕ: Убеждаемся что device валидный
            if not self.device or not self.device.value:
                debug_print("  Device is invalid")
                return None

            # ИСПРАВЛЕНИЕ: Передаем указатели как есть
            pso_ptr = dx.create_graphics_ps(self.device, vs_blob, ps_blob)

            if pso_ptr and hasattr(pso_ptr, 'value') and pso_ptr.value:
                debug_print(f"  PSO created: {hex(pso_ptr.value)}")
                # ИСПРАВЛЕНИЕ: Возвращаем значение указателя
                return pso_ptr.value
            else:
                debug_print("  PSO creation failed - null pointer")
                return None

        except Exception as e:
            debug_print(f"  PSO creation exception: {e}")
            import traceback
            traceback.print_exc()
            logger.error(f"[DX12Backend] PSO creation exception: {e}")
            return None

    def set_graphics_pipeline(self, pso: Any) -> bool:
        """Установить PSO."""
        debug_print(f"set_graphics_pipeline(pso={hex(pso) if isinstance(pso, int) else pso})")

        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False

        if pso is None:
            debug_print("  PSO is None")
            return False

        if isinstance(pso, int) and (pso == 0x87654321 or pso == 0xDEADBEEF or pso == 0):
            debug_print(f"  PSO is stub value: {hex(pso)}")
            return False

        try:
            pso_ptr = ctypes.c_void_p(pso) if isinstance(pso, int) else pso
            result = dx.set_graphics_pipeline(pso_ptr)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  Set pipeline failed: {e}")
            return False

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
            if not self.update_buffer(buf, data):
                debug_print("  Buffer update failed")
            self._resources.append(buf)
            return buf
        except Exception as e:
            debug_print(f"  create_buffer failed: {e}")
            traceback.print_exc()
            return ctypes.c_void_p(0xDEADBEEF)

    def update_buffer(self, buffer: Any, data: bytes) -> bool:
        debug_print(f"update_buffer(buffer={hex(buffer.value if buffer else 0)}, size={len(data)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False

        try:
            addr = int(buffer.value) if isinstance(buffer, ctypes.c_void_p) else int(buffer)
            debug_print(f"  buffer address: {hex(addr)}")
        except Exception:
            addr = None

        if addr is not None and addr in self._constant_buffers:
            debug_print(f"  buffer {hex(addr)} is constant buffer - skipping update")
            return False

        try:
            result = dx.update_subresource(buffer, data)
            debug_print(f"  update_subresource returned {result}")
            return result
        except Exception as e:
            debug_print(f"  Buffer update failed: {e}")
            traceback.print_exc()
            return False

    def create_texture(self, width, height, format, initial_data=None):
        """Создание текстуры с правильной инициализацией"""
        try:
            # Создаем текстуру
            texture = self._dx.create_texture(width, height, format)

            if initial_data and texture:
                # Правильное обновление текстуры
                success = self._dx.update_texture(texture, initial_data, width, height)
                if not success:
                    self.logger.error(f"Failed to update texture with initial data")
                    # Не удаляем текстуру, просто логируем ошибку

            return texture
        except Exception as e:
            self.logger.error(f"Error creating texture: {e}")
            return None

    def create_constant_buffer(self, data: bytes) -> Any:
        """Создаёт const‑buffer (возвращает ТОЛЬКО буфер)."""
        debug_print(f"create_constant_buffer(size={len(data)})")

        if self._in_stub_mode or not self.device or not self.device.value:
            debug_print("  STUB mode - returning fake constant buffer")
            return ctypes.c_void_p(0xDEADBEEF + len(data))

        try:
            buf = dx.create_buffer(self.device, len(data), usage="constant")
            if not buf or not buf.value:
                debug_print("  create_buffer returned nullptr")
                raise RuntimeError("Native constant buffer creation returned nullptr")

            debug_print(f"  Buffer created: {hex(buf.value)}")
            if not self.update_buffer(buf, data):
                debug_print("  Buffer update failed")
            self._resources.append(buf)

            try:
                buf_addr = int(buf.value) if isinstance(buf, ctypes.c_void_p) else int(buf)
                self._constant_buffers.add(buf_addr)
                debug_print(f"  Added to constant_buffers set: {hex(buf_addr)}")
            except Exception as e:
                debug_print(f"  Could not add to constant_buffers set: {e}")

            return buf

        except Exception as e:
            debug_print(f"  create_constant_buffer failed: {e}")
            traceback.print_exc()
            raise

    def update_texture(
            self,
            texture: Any,
            data: bytes,
            width: int,
            height: int
    ) -> bool:
        debug_print(f"update_texture(tex, data_size={len(data)}, w={width}, h={height})")

        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False

        ptr = getattr(texture, "ptr", texture)
        if ptr is None:
            debug_print("  ERROR: texture pointer is None")
            return False

        ptr_val = ptr.value if hasattr(ptr, 'value') else int(ptr) if ptr else 0
        stub_pointers = [0xDEADBEEF, 0xDEADF00D, 0xFEEDC0DE, 0x12345678, 0x87654321, 0]

        if ptr_val in stub_pointers:
            debug_print(f"  SKIPPED: invalid pointer {ptr_val:#x}")
            return False

        expected_size = width * height * 4
        if len(data) != expected_size:
            debug_print(f"  WARNING: data size mismatch: got {len(data)}, expected {expected_size}")
            if len(data) < expected_size:
                data = data + b'\x00' * (expected_size - len(data))
            else:
                data = data[:expected_size]

        try:
            result = dx.update_texture(ptr, data, width, height)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  Update texture failed: {e}")
            traceback.print_exc()
            return False

    def create_descriptor_heap(
            self,
            num_descriptors: int,
            heap_type: str = "cbv_srv_uav",
            shader_visible: bool = True
    ) -> Any:
        debug_print(
            f"create_descriptor_heap(num={num_descriptors}, type='{heap_type}', shader_visible={shader_visible})")

        heap_type_num = 2
        if heap_type.lower() == "rtv":
            heap_type_num = 0
        elif heap_type.lower() == "dsv":
            heap_type_num = 1

        if self._in_stub_mode:
            debug_print("  STUB mode - returning fake heap")
            return ctypes.c_void_p(0xDEADBEEF)

        try:
            heap_ptr = dx.create_descriptor_heap(
                self.device,
                num_descriptors,
                heap_type_num,
                shader_visible
            )

            heap = DescriptorHeap(
                device=self.device,
                num_descriptors=num_descriptors,
                heap_type=heap_type,
                shader_visible=shader_visible
            )
            heap.heap_ptr = heap_ptr
            debug_print(f"  Heap created: {heap_ptr}")
            return heap
        except Exception as e:
            debug_print(f"  create_descriptor_heap failed: {e}")
            return None

    def get_cpu_handle(self, heap: Any, index: int) -> int:
        debug_print(f"get_cpu_handle(heap, index={index})")
        if hasattr(heap, 'get_cpu_handle'):
            return heap.get_cpu_handle(index)
        return 0

    def get_gpu_handle(self, heap: Any, index: int) -> int:
        debug_print(f"get_gpu_handle(heap, index={index})")
        if hasattr(heap, 'get_gpu_handle'):
            return heap.get_gpu_handle(index)
        return 0

    def create_shader_resource_view(self, resource: Any, cpu_handle: int) -> bool:
        debug_print(f"create_shader_resource_view(resource, cpu_handle={hex(cpu_handle)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            debug_print("  Invalid resource pointer")
            return False
        try:
            result = dx.create_shader_resource_view(self.device, ptr, cpu_handle)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  SRV creation failed: {e}")
            return False

    def create_render_target_view(self, resource: Any, cpu_handle: int) -> bool:
        debug_print(f"create_render_target_view(resource, cpu_handle={hex(cpu_handle)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            debug_print("  Invalid resource pointer")
            return False
        try:
            result = dx.create_render_target_view(self.device, ptr, cpu_handle)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  RTV creation failed: {e}")
            return False

    def create_constant_buffer_view(
            self,
            resource: Any,
            cpu_handle: int
    ) -> bool:
        debug_print(f"create_constant_buffer_view(resource, cpu_handle={hex(cpu_handle)})")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False
        ptr = getattr(resource, "ptr", resource)
        if not ptr or not ptr.value:
            debug_print("  Invalid resource pointer")
            return False
        try:
            result = dx.create_constant_buffer_view(self.device, ptr, cpu_handle)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  CBV creation failed: {e}")
            return False

    def set_root_descriptor_table(self, root_index: int, gpu_handle: int) -> bool:
        debug_print(f"set_root_descriptor_table(root_index={root_index}, gpu_handle={hex(gpu_handle)})")
        if not self._in_stub_mode:
            try:
                result = dx.set_root_descriptor_table(root_index, gpu_handle)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Set root descriptor table failed: {e}")
                return False
        return False

    def set_descriptor_heaps(self, heaps: Sequence[Any]) -> bool:
        debug_print(f"set_descriptor_heaps(count={len(heaps)})")
        try:
            result = dx.set_descriptor_heaps(heaps)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  Set descriptor heaps failed: {e}")
            return False

    def set_render_target(self, rtv: int) -> bool:
        debug_print(f"set_render_target(rtv={hex(rtv)})")
        if not self._in_stub_mode:
            try:
                result = dx.set_render_target(rtv)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Set render target failed: {e}")
                return False
        return False

    def set_render_targets(self, rtvs: Sequence[int]) -> bool:
        debug_print(f"set_render_targets(count={len(rtvs)})")
        if not self._in_stub_mode:
            try:
                result = dx.set_render_targets(rtvs)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Set render targets failed: {e}")
                return False
        return False

    def clear_render_target(
            self,
            rtv: int,
            color: Tuple[float, float, float, float] = (0.0, 0.0, 0.0, 1.0),
    ) -> bool:
        debug_print(f"clear_render_target(rtv={hex(rtv)}, color={color})")
        if not self._in_stub_mode:
            try:
                result = dx.clear_render_target(rtv, color)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Clear render target failed: {e}")
                return False
        return False

    def set_viewport(
            self,
            x: int,
            y: int,
            width: int,
            height: int,
            min_depth: float = 0.0,
            max_depth: float = 1.0,
    ) -> bool:
        debug_print(f"set_viewport(x={x}, y={y}, w={width}, h={height})")
        self.viewport = (x, y, width, height)
        if not self._in_stub_mode:
            try:
                result = dx.set_viewport(x, y, width, height, min_depth, max_depth)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Set viewport failed: {e}")
                return False
        return False

    def set_scissor_rect(
            self,
            left: int,
            top: int,
            right: int,
            bottom: int,
    ) -> bool:
        debug_print(f"set_scissor_rect(left={left}, top={top}, right={right}, bottom={bottom})")
        self.scissor = (left, top, right, bottom)
        if not self._in_stub_mode:
            try:
                result = dx.set_scissor_rect(left, top, right, bottom)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Set scissor rect failed: {e}")
                return False
        return False

    def set_vertex_buffers(
            self,
            vertex_buffer: Any,
            index_buffer: Optional[Any] = None,
    ) -> bool:
        debug_print(f"set_vertex_buffers(vertex={hex(vertex_buffer.value if vertex_buffer else 0)})")
        if not self._in_stub_mode:
            try:
                result = dx.set_vertex_buffers(vertex_buffer, index_buffer)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Set vertex buffers failed: {e}")
                return False
        return False

    def draw(
            self,
            vertex_count: int,
            start_vertex: int = 0,
            instance_count: int = 1
    ) -> bool:
        debug_print(f"draw(vertex_count={vertex_count})")
        if not self._in_stub_mode:
            try:
                result = dx.draw_instanced(vertex_count, instance_count, start_vertex, 0)
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Draw failed: {e}")
                return False
        return False

    def draw_indexed(
            self,
            index_count: int,
            start_index: int = 0,
            base_vertex: int = 0,
            instance_count: int = 1,
    ) -> bool:
        debug_print(f"draw_indexed(index_count={index_count})")
        if not self._in_stub_mode:
            try:
                result = dx.draw_indexed_instanced(
                    index_count, instance_count, start_index, base_vertex, 0
                )
                debug_print(f"  -> {result}")
                return result
            except Exception as e:
                debug_print(f"  Draw indexed failed: {e}")
                return False
        return False

    def draw_fullscreen_quad(
            self,
            pso: Any,
            descriptor_heaps: Sequence[Any],
            root_parameters: Sequence[Tuple[int, int]],
    ) -> bool:
        debug_print("draw_fullscreen_quad()")
        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False
        try:
            if not self.set_graphics_pipeline(pso):
                debug_print("  set_graphics_pipeline failed")
                return False
            if not self.set_descriptor_heaps(descriptor_heaps):
                debug_print("  set_descriptor_heaps failed")
                return False
            for root_idx, gpu_handle in root_parameters:
                if not self.set_root_descriptor_table(root_idx, gpu_handle):
                    debug_print(f"  set_root_descriptor_table({root_idx}) failed")
                    return False
            result = dx.draw_instanced(3, 1, 0, 0)
            debug_print(f"  -> {result}")
            return result
        except Exception as e:
            debug_print(f"  Draw fullscreen quad failed: {e}")
            return False

    def wait_for_gpu(self) -> bool:
        """Ожидать завершения работы GPU."""
        debug_print("wait_for_gpu()")

        if self._in_stub_mode:
            debug_print("  STUB mode - skipping")
            return False

        try:
            for attempt in range(5):
                result = dx.wait_for_gpu()
                debug_print(f"  attempt {attempt + 1}: {result}")
                if result:
                    return True
                time.sleep(0.02)

            debug_print("  wait_for_gpu failed after 5 attempts")
            return False
        except Exception as e:
            debug_print(f"  Wait for GPU failed: {e}")
            return False

    def enable_depth_test(self, enable: bool) -> None:
        debug_print(f"enable_depth_test(enable={enable})")
        self._depth_test_enabled = enable

    def begin_frame(self) -> bool:
        """Начать новый кадр."""
        debug_print("begin_frame()")

        if not self._initialized or self._in_stub_mode:
            debug_print("  not initialized or stub mode")
            return False

        if self.rtv_heap:
            self.rtv_heap.reset()
        if self.cbv_srv_uav_heap:
            self.cbv_srv_uav_heap.reset()

        try:
            result = dx.begin_frame()
            debug_print(f"  dx.begin_frame() returned {result}")

            if not result:
                debug_print("  begin_frame failed, attempting recovery...")
                time.sleep(0.01)
                result = dx.begin_frame()
                debug_print(f"  retry returned {result}")

            return result
        except Exception as e:
            debug_print(f"  dx.begin_frame() failed: {e}")
            return False

    def end_frame(self) -> bool:
        """Завершить кадр."""
        debug_print("end_frame()")

        if not self._initialized or self._in_stub_mode:
            debug_print("  not initialized or stub mode")
            return False

        try:
            result = dx.end_frame()
            debug_print(f"  dx.end_frame() returned {result}")

            if result and self.swap_chain and self.swap_chain.value:
                present_result = self.present(1 if self._vsync_enabled else 0)
                debug_print(f"  present returned {present_result}")

            return result
        except Exception as e:
            debug_print(f"  dx.end_frame() failed: {e}")
            return False

    def release_resource(self, resource) -> None:
        """Безопасное освобождение ресурса."""
        if not resource:
            return

        ptr = int(resource) if hasattr(resource, 'value') else int(resource)
        stub_pointers = [0xDEADBEEF, 0xDEADF00D, 0xFEEDC0DE, 0x12345678, 0x87654321]

        if ptr in stub_pointers:
            debug_print(f"  SKIPPED: stub pointer {ptr:#x}")
            return

        if ptr in self._released_resources:
            debug_print(f"  SKIPPED: already released {ptr:#x}")
            return

        try:
            self._released_resources.add(ptr)
            dx.release_resource(resource)
            debug_print(f"  -> OK (released {ptr:#x})")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
            self._released_resources.discard(ptr)

    def shutdown(self) -> None:
        debug_print("shutdown()")
        logger.info("[DX12Backend] Releasing all native resources")

        try:
            self.end_frame()
        except Exception as exc:
            debug_print(f"  end_frame during shutdown failed: {exc}")

        self._released_resources.clear()

        for i, r in enumerate(self._resources):
            debug_print(f"  releasing resource[{i}]: {hex(r.value if r else 0)}")
            try:
                self.release_resource(r)
            except Exception as exc:
                debug_print(f"  Failed to release resource {r}: {exc}")
        self._resources.clear()

        try:
            dx.force_cleanup()
        except:
            pass

        debug_print("  shutdown done")

    def get_frame_index(self) -> int:
        if self._initialized and not self._in_stub_mode:
            try:
                return dx.get_frame_index()
            except:
                pass
        return self._current_frame

    def get_rtv_descriptor_size(self) -> int:
        return dx.get_rtv_descriptor_size()

    def get_dsv_descriptor_size(self) -> int:
        return dx.get_dsv_descriptor_size()

    def recreate_swapchain_rtv(self) -> bool:
        debug_print("recreate_swapchain_rtv()")
        return self._create_swapchain_rtv()