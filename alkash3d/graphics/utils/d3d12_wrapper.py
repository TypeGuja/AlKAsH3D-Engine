# -*- coding: utf-8 -*-
"""
Thin‑wrapper over the Rust crate ``alkash3d_dx12``.
"""

from __future__ import annotations

import ctypes
import os
import sys
from pathlib import Path
from typing import Callable, Tuple, Any, Optional, Union
from alkash3d.graphics.utils import *

DEBUG = True

def debug_print(*args, **kwargs):
    if DEBUG:
        print(*args, **kwargs)

try:
    from alkash3d.utils.logger import logger
except Exception:
    logger = None

_ext = ".dll" if sys.platform.startswith("win") else ".so"
_lib_path = Path(__file__).with_name(f"alkash3d_dx12{_ext}")

if not _lib_path.is_file():
    raise RuntimeError(
        f"[d3d12_wrapper] Native library not found: {_lib_path}"
    )

if sys.platform.startswith("win"):
    _lib = ctypes.CDLL(str(_lib_path))
else:
    _lib = ctypes.CDLL(str(_lib_path))

if logger:
    logger.debug(f"[d3d12_wrapper] Loaded library from: {_lib_path}")

if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

SWAP_CHAIN_BUFFER_COUNT = 2
DXGI_FORMAT_R8G8B8A8_UNORM = 28

def _load_func(
    name: str,
    restype,
    argtypes,
    *,
    required: bool = False,
) -> Optional[Callable]:
    try:
        fn = getattr(_lib, name)
        fn.restype = restype
        fn.argtypes = argtypes
        if logger:
            logger.debug(f"[d3d12_wrapper] Loaded function '{name}'")
        return fn
    except AttributeError as e:
        if required:
            raise RuntimeError(
                f"[d3d12_wrapper] Required function '{name}' not exported from '{_lib_path}'"
            ) from e
        if logger:
            logger.debug(f"[d3d12_wrapper] Function '{name}' not found, skipping")
        return None

# Load all functions ---------------------------------------------------------
_create_device = _load_func("create_device", ctypes.c_void_p, [])
_create_command_queue = _load_func("create_command_queue", ctypes.c_void_p, [ctypes.c_void_p])

_create_swap_chain = _load_func(
    "create_swap_chain",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uintptr, ctypes.c_uint, ctypes.c_uint],
)

_swap_chain_get_buffer = _load_func(
    "swap_chain_get_buffer",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint]
)

_resize_swap_chain = _load_func("resize_swap_chain", None, [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint])
_present_swap_chain = _load_func("present_swap_chain", None, [ctypes.c_void_p, ctypes.c_uint])

_compile_shader = _load_func(
    "compile_shader",
    ctypes.c_int,
    [ctypes.c_wchar_p, ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)],
    required=True,
)

_create_graphics_ps = _load_func(
    "create_graphics_ps", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
)
_set_graphics_pipeline = _load_func("set_graphics_pipeline", None, [ctypes.c_void_p])

_create_buffer = _load_func(
    "create_buffer", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p]
)
_update_subresource = _load_func(
    "update_subresource", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
)

_create_texture_from_memory = _load_func(
    "create_texture_from_memory",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_char_p],
)
_update_texture = _load_func(
    "update_texture", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]
)

_create_descriptor_heap = _load_func(
    "create_descriptor_heap", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]
)
_GetCPUDescriptorHandleForHeapStart = _load_func(
    "GetCPUDescriptorHandleForHeapStart", ctypes.c_uintptr, [ctypes.c_void_p]
)
_GetGPUDescriptorHandleForHeapStart = _load_func(
    "GetGPUDescriptorHandleForHeapStart", ctypes.c_uintptr, [ctypes.c_void_p]
)
_offset_descriptor_handle = _load_func(
    "offset_descriptor_handle", ctypes.c_uintptr, [ctypes.c_uintptr, ctypes.c_uint]
)

_create_shader_resource_view = _load_func(
    "create_shader_resource_view", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
)
_create_render_target_view = _load_func(
    "create_render_target_view", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
)

_set_root_descriptor_table = _load_func(
    "set_root_descriptor_table", None, [ctypes.c_uint, ctypes.c_uintptr]
)
_set_descriptor_heaps = _load_func(
    "set_descriptor_heaps", None, [ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
)

_set_render_target = _load_func("set_render_target", None, [ctypes.c_uintptr])
_set_render_targets = _load_func(
    "set_render_targets", None, [ctypes.c_size_t, ctypes.POINTER(ctypes.c_uintptr)]
)
_clear_render_target = _load_func(
    "clear_render_target", None, [ctypes.c_uintptr, ctypes.POINTER(ctypes.c_float)]
)

_set_viewport = _load_func(
    "set_viewport",
    None,
    [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_float, ctypes.c_float],
)
_set_scissor_rect = _load_func(
    "set_scissor_rect", None, [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int]
)

_set_vertex_buffers = _load_func(
    "set_vertex_buffers", None, [ctypes.c_void_p, ctypes.c_void_p]
)
_draw_instanced = _load_func(
    "draw_instanced", None, [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
)
_draw_indexed_instanced = _load_func(
    "draw_indexed_instanced",
    None,
    [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_uint],
)

_wait_for_gpu = _load_func("wait_for_gpu", None, [])
_release_resource = _load_func("release_resource", None, [ctypes.c_void_p])
_get_frame_index = _load_func("get_frame_index", ctypes.c_uint, [])
_get_rtv_descriptor_size = _load_func("get_rtv_descriptor_size", ctypes.c_uint, [])
_get_dsv_descriptor_size = _load_func("get_dsv_descriptor_size", ctypes.c_uint, [])

# ----------------------------------------------------------------------
# Public API with error handling
# ----------------------------------------------------------------------
def create_device() -> ctypes.c_void_p:
    if _create_device:
        result = _create_device()
        return ctypes.c_void_p(result) if result else ctypes.c_void_p()
    return ctypes.c_void_p(0xDEADBEEF)

def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    if _create_command_queue and device:
        result = _create_command_queue(device)
        return ctypes.c_void_p(result) if result else ctypes.c_void_p()
    return ctypes.c_void_p(0xDEADBEEF)

def create_swap_chain(
    command_queue: ctypes.c_void_p,
    hwnd: int,
    width: int,
    height: int,
) -> ctypes.c_void_p:
    if _create_swap_chain and command_queue and hwnd:
        result = _create_swap_chain(
            command_queue,
            ctypes.c_uintptr(hwnd),
            ctypes.c_uint(width),
            ctypes.c_uint(height),
        )
        return ctypes.c_void_p(result) if result else ctypes.c_void_p()
    return ctypes.c_void_p()

def resize_swap_chain(swap_chain: ctypes.c_void_p, width: int, height: int) -> None:
    if _resize_swap_chain and swap_chain:
        _resize_swap_chain(swap_chain, ctypes.c_uint(width), ctypes.c_uint(height))

def present_swap_chain(swap_chain: ctypes.c_void_p, sync_interval: int = 1) -> None:
    if _present_swap_chain and swap_chain and swap_chain.value:
        _present_swap_chain(swap_chain, ctypes.c_uint(sync_interval))

def compile_shader(self, shader_type: str, source_path: str) -> int:
    if shader_type == "vs":
        entry = "VSMain"
        profile = "vs_5_0"
    elif shader_type == "ps":
        entry = "PSMain"
        profile = "ps_5_0"
    else:
        entry = "main"
        profile = "vs_5_0" if "vert" in source_path else "ps_5_0"

    if not os.path.exists(source_path):
        raise FileNotFoundError(f"Shader file not found: {source_path}")

    result = dx.compile_hlsl(source_path, entry, profile)
    return result

def compile_hlsl(source_path: str, entry_point: str, profile: str) -> int:
    if not _compile_shader:
        raise RuntimeError("Shader compiler not available")
    if not os.path.isfile(source_path):
        raise FileNotFoundError(f"Shader file not found: {source_path}")

    src_utf16 = os.path.abspath(source_path)

    entry_c = ctypes.c_char_p(entry_point.encode('utf-8'))
    profile_c = ctypes.c_char_p(profile.encode('utf-8'))
    out_blob = ctypes.c_void_p()

    hr = _compile_shader(src_utf16, entry_c, profile_c, ctypes.byref(out_blob))

    if hr != 0:
        raise RuntimeError(f"Shader compilation failed with HRESULT {hr}")

    if not out_blob.value:
        raise RuntimeError("Shader compilation returned null blob")

    return out_blob.value

def create_graphics_ps(
        device: ctypes.c_void_p,
        vs_blob: ctypes.c_void_p,
        ps_blob: ctypes.c_void_p,
) -> ctypes.c_void_p:
    if _create_graphics_ps and device and vs_blob and ps_blob:
        if not vs_blob.value or not ps_blob.value:
            print("[WARNING] Invalid shader blobs")
            return ctypes.c_void_p(0xFEEDC0DE)

        result = _create_graphics_ps(device, vs_blob, ps_blob)
        return ctypes.c_void_p(result) if result else ctypes.c_void_p(0xFEEDC0DE)
    return ctypes.c_void_p(0xFEEDC0DE)

def set_graphics_pipeline(pso: ctypes.c_void_p) -> None:
    if _set_graphics_pipeline and pso:
        _set_graphics_pipeline(pso)
        _set_graphics_pipeline(pso)

def swap_chain_get_buffer(swap_chain: ctypes.c_void_p, buffer_index: int) -> ctypes.c_void_p:
    if _swap_chain_get_buffer is None:
        debug_print("[WARNING] swap_chain_get_buffer function not found")
        return ctypes.c_void_p()

    if not swap_chain or not swap_chain.value:
        debug_print("[WARNING] Invalid swap chain")
        return ctypes.c_void_p()

    result = _swap_chain_get_buffer(swap_chain, ctypes.c_uint(buffer_index))
    return ctypes.c_void_p(result) if result else ctypes.c_void_p()

def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    if _create_buffer and device and size > 0:
        if isinstance(usage, bytes):
            usage_bytes = usage
        else:
            usage_bytes = usage.encode("utf-8")
        result = _create_buffer(device, ctypes.c_size_t(size), ctypes.c_char_p(usage_bytes))
        return ctypes.c_void_p(result) if result else ctypes.c_void_p()
    return ctypes.c_void_p(0xDEADBEEF)

def update_subresource(buffer: Any, data: bytes) -> None:
    if not _update_subresource:
        return
    if isinstance(buffer, int):
        buffer_ptr = ctypes.c_void_p(buffer)
    else:
        buffer_ptr = buffer
    if not buffer_ptr or not buffer_ptr.value:
        return
    sz = len(data)
    raw = ctypes.create_string_buffer(data, sz)
    data_ptr = ctypes.c_void_p(ctypes.addressof(raw))
    _update_subresource(buffer_ptr, data_ptr, ctypes.c_size_t(sz))

def create_texture_from_memory(
        device: ctypes.c_void_p,
        data: Optional[Union[bytes, ctypes.c_void_p]],
        width: int,
        height: int,
        format: str = "rgba8",
) -> ctypes.c_void_p:
    if not _create_texture_from_memory or not device:
        return ctypes.c_void_p(0xDEADBEEF + width + height)

    if data is not None:
        if isinstance(data, (bytes, bytearray)):
            buffer = ctypes.create_string_buffer(data, len(data))
            data_ptr = ctypes.c_void_p(ctypes.addressof(buffer))
        elif isinstance(data, ctypes.c_void_p):
            data_ptr = data
        else:
            data_ptr = ctypes.c_void_p()
    else:
        data_ptr = ctypes.c_void_p()

    if isinstance(format, str):
        fmt_bytes = format.encode('utf-8')
    else:
        fmt_bytes = format

    result = _create_texture_from_memory(
        device,
        data_ptr,
        ctypes.c_uint(width),
        ctypes.c_uint(height),
        ctypes.c_char_p(fmt_bytes),
    )
    return ctypes.c_void_p(result) if result else ctypes.c_void_p()

def update_texture(texture: ctypes.c_void_p, data: Union[bytes, ctypes.c_void_p], width: int, height: int) -> None:
    if not _update_texture or not texture:
        return
    data_ptr = ctypes.c_void_p()
    if data is not None:
        if isinstance(data, (bytes, bytearray)):
            raw = ctypes.create_string_buffer(data, len(data))
            data_ptr = ctypes.c_void_p(ctypes.addressof(raw))
        elif isinstance(data, ctypes.c_void_p):
            data_ptr = data
    _update_texture(texture, data_ptr, ctypes.c_uint(width), ctypes.c_uint(height))

def create_descriptor_heap(
    device: ctypes.c_void_p,
    num_descriptors: int,
    heap_type: int,
) -> ctypes.c_void_p:
    if _create_descriptor_heap and device and num_descriptors > 0:
        result_ptr = _create_descriptor_heap(
            device,
            ctypes.c_uint(num_descriptors),
            ctypes.c_uint(heap_type),
        )
        return ctypes.c_void_p(result_ptr) if result_ptr else ctypes.c_void_p()
    return ctypes.c_void_p()

def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if _GetCPUDescriptorHandleForHeapStart and heap and heap.value:
        return _GetCPUDescriptorHandleForHeapStart(heap)
    return 0

def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if _GetGPUDescriptorHandleForHeapStart and heap and heap.value:
        return _GetGPUDescriptorHandleForHeapStart(heap)
    return 0

def offset_descriptor_handle(base: int, index: int) -> int:
    if _offset_descriptor_handle:
        if isinstance(base, ctypes.c_void_p):
            base = base.value
        return _offset_descriptor_handle(ctypes.c_uintptr(base), ctypes.c_uint(index))
    return base + index * 32

def create_shader_resource_view(
    device: ctypes.c_void_p,
    resource: ctypes.c_void_p,
    cpu_handle: int,
) -> None:
    if _create_shader_resource_view and device and resource:
        _create_shader_resource_view(device, resource, ctypes.c_void_p(cpu_handle))

def create_render_target_view(
    device: ctypes.c_void_p,
    resource: ctypes.c_void_p,
    cpu_handle: int,
) -> None:
    if _create_render_target_view and device and resource and cpu_handle != 0:
        _create_render_target_view(device, resource, ctypes.c_void_p(cpu_handle))

def set_root_descriptor_table(root_index: int, gpu_handle: int) -> None:
    if _set_root_descriptor_table:
        _set_root_descriptor_table(ctypes.c_uint(root_index), ctypes.c_uintptr(gpu_handle))

def set_descriptor_heaps(heaps: Tuple[ctypes.c_void_p, ...]) -> None:
    if _set_descriptor_heaps and heaps:
        count = len(heaps)
        array_type = ctypes.c_void_p * count
        _set_descriptor_heaps(ctypes.c_size_t(count), array_type(*heaps))

def set_render_target(rtv: int) -> None:
    if _set_render_target:
        _set_render_target(ctypes.c_uintptr(rtv))

def set_render_targets(rtvs: Tuple[int, ...]) -> None:
    if _set_render_targets and rtvs:
        count = len(rtvs)
        array_type = ctypes.c_uintptr * count
        _set_render_targets(
            ctypes.c_size_t(count),
            array_type(*[ctypes.c_uintptr(r) for r in rtvs]),
        )

def clear_render_target(rtv: int, color: Tuple[float, float, float, float]) -> None:
    if _clear_render_target:
        rgba = (ctypes.c_float * 4)(*color)
        _clear_render_target(ctypes.c_uintptr(rtv), rgba)

def set_viewport(
    x: int,
    y: int,
    width: int,
    height: int,
    min_depth: float = 0.0,
    max_depth: float = 1.0,
) -> None:
    if _set_viewport:
        _set_viewport(
            ctypes.c_int(x),
            ctypes.c_int(y),
            ctypes.c_int(width),
            ctypes.c_int(height),
            ctypes.c_float(min_depth),
            ctypes.c_float(max_depth),
        )

def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> None:
    if _set_scissor_rect:
        _set_scissor_rect(
            ctypes.c_int(left),
            ctypes.c_int(top),
            ctypes.c_int(right),
            ctypes.c_int(bottom),
        )

def set_vertex_buffers(
    vertex_buffer: ctypes.c_void_p,
    index_buffer: Optional[ctypes.c_void_p] = None,
) -> None:
    if _set_vertex_buffers:
        ib = index_buffer if index_buffer is not None else ctypes.c_void_p()
        _set_vertex_buffers(vertex_buffer, ib)

def draw_instanced(
    vertex_count: int,
    instance_count: int = 1,
    start_vertex: int = 0,
    start_instance: int = 0,
) -> None:
    if _draw_instanced:
        _draw_instanced(
            ctypes.c_uint(vertex_count),
            ctypes.c_uint(instance_count),
            ctypes.c_uint(start_vertex),
            ctypes.c_uint(start_instance),
        )

def draw_indexed_instanced(
    index_count: int,
    instance_count: int = 1,
    start_index: int = 0,
    base_vertex: int = 0,
    start_instance: int = 0,
) -> None:
    if _draw_indexed_instanced:
        _draw_indexed_instanced(
            ctypes.c_uint(index_count),
            ctypes.c_uint(instance_count),
            ctypes.c_uint(start_index),
            ctypes.c_int(base_vertex),
            ctypes.c_uint(start_instance),
        )

def wait_for_gpu() -> None:
    if _wait_for_gpu:
        _wait_for_gpu()

def release_resource(resource: Any) -> None:
    if not _release_resource:
        return
    if isinstance(resource, int):
        ptr = ctypes.c_void_p(resource)
    elif isinstance(resource, ctypes.c_void_p):
        ptr = resource
    else:
        try:
            ptr = ctypes.c_void_p(int(resource))
        except:
            return
    if not ptr or not ptr.value:
        return
    stub_values = [0xDEADBEEF, 0xDEADF00D, 0xFEEDC0DE, 0x12345678, 0x87654321]
    if ptr.value in stub_values:
        return
    try:
        _release_resource(ptr)
    except Exception:
        pass

def get_frame_index() -> int:
    if _get_frame_index:
        return _get_frame_index()
    return 0

def get_rtv_descriptor_size() -> int:
    if _get_rtv_descriptor_size:
        return _get_rtv_descriptor_size()
    return 32

def get_dsv_descriptor_size() -> int:
    if _get_dsv_descriptor_size:
        return _get_dsv_descriptor_size()
    return 32

__all__ = [
    "SWAP_CHAIN_BUFFER_COUNT",
    "DXGI_FORMAT_R8G8B8A8_UNORM",
    "create_device",
    "create_command_queue",
    "create_swap_chain",
    "resize_swap_chain",
    "present_swap_chain",
    "compile_shader",
    "compile_hlsl",
    "create_graphics_ps",
    "set_graphics_pipeline",
    "create_buffer",
    "update_subresource",
    "create_texture_from_memory",
    "update_texture",
    "create_descriptor_heap",
    "GetCPUDescriptorHandleForHeapStart",
    "GetGPUDescriptorHandleForHeapStart",
    "offset_descriptor_handle",
    "create_shader_resource_view",
    "create_render_target_view",
    "set_root_descriptor_table",
    "set_descriptor_heaps",
    "set_render_target",
    "set_render_targets",
    "clear_render_target",
    "set_viewport",
    "set_scissor_rect",
    "set_vertex_buffers",
    "draw_instanced",
    "draw_indexed_instanced",
    "wait_for_gpu",
    "release_resource",
    "get_frame_index",
    "get_rtv_descriptor_size",
    "get_dsv_descriptor_size",
    "DEBUG",
]