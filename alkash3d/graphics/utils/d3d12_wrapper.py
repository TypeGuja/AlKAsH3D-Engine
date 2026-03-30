# alkash3d/graphics/utils/d3d12_wrapper.py
# -*- coding: utf-8 -*-
"""
Thin ctypes wrapper around the native alkash3d_dx12 DLL.
"""

from __future__ import annotations
import ctypes
import os
import sys
import threading
import atexit
from pathlib import Path
from typing import Any, Optional, Sequence, Tuple

from alkash3d.utils import logger

# ----------------------------------------------------------------------
# Compatibility
# ----------------------------------------------------------------------
if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print("[D3D12_WRAPPER]", *args, **kwargs)


# ----------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------
SWAP_CHAIN_BUFFER_COUNT = 2
DXGI_FORMAT_R8G8B8A8_UNORM = 28

_ext = ".dll" if sys.platform.startswith("win") else ".so"
_DEFAULT_DLL_NAME = f"alkash3d_dx12{_ext}"

_lib: Optional[ctypes.CDLL] = None
_lib_path: Optional[str] = None

_PTR_CACHE = {}
_PTR_CACHE_LOCK = threading.RLock()

_TEMP_BUFFERS = []
_TEMP_BUFFERS_LOCK = threading.RLock()


def _get_cached_void_p(ptr_value: int) -> ctypes.c_void_p:
    """Возвращает кэшированный ctypes.c_void_p для указателя."""
    if ptr_value == 0:
        return ctypes.c_void_p(0)

    with _PTR_CACHE_LOCK:
        if ptr_value not in _PTR_CACHE:
            _PTR_CACHE[ptr_value] = ctypes.c_void_p(ptr_value)
        return _PTR_CACHE[ptr_value]


def _cleanup_temp_buffers():
    with _TEMP_BUFFERS_LOCK:
        _TEMP_BUFFERS.clear()


atexit.register(_cleanup_temp_buffers)


def _locate_dll() -> Optional[str]:
    global _lib_path
    if _lib_path:
        return _lib_path

    base_dir = Path(__file__).parent
    candidates = [
        base_dir / _DEFAULT_DLL_NAME,
        base_dir.parent.parent / _DEFAULT_DLL_NAME,
        Path.cwd() / _DEFAULT_DLL_NAME,
        Path(_DEFAULT_DLL_NAME),
    ]

    for cand in candidates:
        if cand.exists():
            _lib_path = str(cand.absolute())
            logger.debug(f"[d3d12_wrapper] Found DLL at: {_lib_path}")
            return _lib_path

    logger.error("[d3d12_wrapper] DLL not found")
    return None


def _load_lib() -> Optional[ctypes.CDLL]:
    global _lib
    if _lib is not None:
        return _lib

    libpath = _locate_dll()
    if libpath is None:
        logger.error("[d3d12_wrapper] Native DLL missing")
        return None

    try:
        _lib = ctypes.CDLL(libpath)
        logger.info(f"[d3d12_wrapper] Loaded native library: {libpath}")
        return _lib
    except Exception as e:
        logger.error(f"[d3d12_wrapper] Failed to load {libpath}: {e}")
        _lib = None
        return None


def _load_func(name, restype, argtypes, required=False):
    lib = _load_lib()
    if lib is None:
        if required:
            raise RuntimeError(f"Required function '{name}' not available")
        return None

    try:
        fn = getattr(lib, name)
        fn.restype = restype
        fn.argtypes = argtypes
        return fn
    except AttributeError:
        if required:
            raise RuntimeError(f"Required function '{name}' missing")
        return None


# ----------------------------------------------------------------------
# Load all exported functions
# ----------------------------------------------------------------------
_create_device = _load_func("create_device", ctypes.c_void_p, [], required=True)
_create_command_queue = _load_func("create_command_queue", ctypes.c_void_p, [ctypes.c_void_p], required=True)
_release_resource = _load_func("release_resource", None, [ctypes.c_void_p], required=False)
_force_cleanup = _load_func("force_cleanup", None, [], required=False)
_create_swap_chain = _load_func("create_swap_chain", ctypes.c_void_p,
                                [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32, ctypes.c_uint32], required=True)
_swap_chain_get_buffer = _load_func("swap_chain_get_buffer", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_uint32],
                                    required=True)
_resize_swap_chain = _load_func("resize_swap_chain", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32],
                                required=True)
_present_swap_chain = _load_func("present_swap_chain", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_uint32],
                                 required=True)
_begin_frame = _load_func("begin_frame", ctypes.c_bool, [], required=True)
_end_frame = _load_func("end_frame", ctypes.c_bool, [], required=True)
_wait_for_gpu = _load_func("wait_for_gpu", ctypes.c_bool, [], required=False)
_get_frame_index = _load_func("get_frame_index", ctypes.c_uint32, [], required=True)

_compile_shader = _load_func(
    "compile_shader",
    ctypes.c_int32,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)],
    required=True,
)

_create_graphics_ps = _load_func("create_graphics_ps", ctypes.c_void_p,
                                 [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p], required=True)
_set_graphics_pipeline = _load_func("set_graphics_pipeline", ctypes.c_bool, [ctypes.c_void_p], required=True)
_create_buffer = _load_func("create_buffer", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p],
                            required=True)
_update_subresource = _load_func("update_subresource", ctypes.c_bool,
                                 [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t], required=True)

_create_texture_from_memory = _load_func(
    "create_texture_from_memory",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p],
    required=True,
)

_update_texture = _load_func("update_texture", ctypes.c_bool,
                             [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32], required=True)
_create_descriptor_heap = _load_func(
    "create_descriptor_heap",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_bool],
    required=True
)
_GetCPUDescriptorHandleForHeapStart = _load_func("GetCPUDescriptorHandleForHeapStart", ctypes.c_uint64,
                                                 [ctypes.c_void_p], required=True)
_GetGPUDescriptorHandleForHeapStart = _load_func("GetGPUDescriptorHandleForHeapStart", ctypes.c_uint64,
                                                 [ctypes.c_void_p], required=True)
_offset_descriptor_handle = _load_func("offset_descriptor_handle", ctypes.c_uint64, [ctypes.c_uint64, ctypes.c_uint32],
                                       required=True)
_create_shader_resource_view = _load_func("create_shader_resource_view", ctypes.c_bool,
                                          [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64], required=True)
_create_render_target_view = _load_func("create_render_target_view", ctypes.c_bool,
                                        [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64], required=True)
_create_constant_buffer_view = _load_func("create_constant_buffer_view", ctypes.c_bool,
                                          [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64], required=True)
_set_root_descriptor_table = _load_func("set_root_descriptor_table", ctypes.c_bool, [ctypes.c_uint32, ctypes.c_uint64],
                                        required=True)
_set_descriptor_heaps = _load_func("set_descriptor_heaps", ctypes.c_bool,
                                   [ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)], required=True)
_set_render_target = _load_func("set_render_target", ctypes.c_bool, [ctypes.c_uint64], required=True)
_set_render_targets = _load_func("set_render_targets", ctypes.c_bool,
                                 [ctypes.c_size_t, ctypes.POINTER(ctypes.c_uint64)], required=True)
_clear_render_target = _load_func("clear_render_target", ctypes.c_bool,
                                  [ctypes.c_uint64, ctypes.POINTER(ctypes.c_float)], required=True)
_set_viewport = _load_func("set_viewport", ctypes.c_bool,
                           [ctypes.c_int32, ctypes.c_int32, ctypes.c_int32, ctypes.c_int32, ctypes.c_float,
                            ctypes.c_float], required=True)
_set_scissor_rect = _load_func("set_scissor_rect", ctypes.c_bool,
                               [ctypes.c_int32, ctypes.c_int32, ctypes.c_int32, ctypes.c_int32], required=True)
_set_vertex_buffers = _load_func("set_vertex_buffers", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p],
                                 required=True)
_draw_instanced = _load_func("draw_instanced", ctypes.c_bool,
                             [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32], required=True)
_draw_indexed_instanced = _load_func("draw_indexed_instanced", ctypes.c_bool,
                                     [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_int32,
                                      ctypes.c_uint32], required=True)
_get_rtv_descriptor_size = _load_func("get_rtv_descriptor_size", ctypes.c_uint32, [], required=True)
_get_dsv_descriptor_size = _load_func("get_dsv_descriptor_size", ctypes.c_uint32, [], required=True)
_set_vsync = _load_func("set_vsync", None, [ctypes.c_bool], required=False)

def _to_cvoid(ptr: Any) -> ctypes.c_void_p:
    if ptr is None:
        return ctypes.c_void_p()
    if isinstance(ptr, ctypes.c_void_p):
        return ptr
    if isinstance(ptr, int):
        return _get_cached_void_p(ptr)
    try:
        return _get_cached_void_p(int(ptr))
    except Exception:
        return ctypes.c_void_p()

# ----------------------------------------------------------------------
# Public API
# ----------------------------------------------------------------------

def create_device() -> ctypes.c_void_p:
    ptr_val = _create_device()
    return _get_cached_void_p(ptr_val)


def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    ptr_val = _create_command_queue(_to_cvoid(device))
    return _get_cached_void_p(ptr_val)


def create_swap_chain(queue: ctypes.c_void_p, hwnd: int, width: int, height: int) -> ctypes.c_void_p:
    ptr_val = _create_swap_chain(
        _to_cvoid(queue),
        ctypes.c_uint64(hwnd),
        ctypes.c_uint32(width),
        ctypes.c_uint32(height),
    )
    return _get_cached_void_p(ptr_val)

def swap_chain_get_buffer(swap: ctypes.c_void_p, idx: int) -> ctypes.c_void_p:
    return _to_cvoid(_swap_chain_get_buffer(_to_cvoid(swap), ctypes.c_uint32(idx)))


def resize_swap_chain(swap: ctypes.c_void_p, w: int, h: int) -> bool:
    return bool(_resize_swap_chain(_to_cvoid(swap), ctypes.c_uint32(w), ctypes.c_uint32(h)))


def present_swap_chain(swap: ctypes.c_void_p, sync_interval: int = 1) -> bool:
    return bool(_present_swap_chain(_to_cvoid(swap), ctypes.c_uint32(sync_interval)))


def begin_frame() -> bool:
    return bool(_begin_frame())


def end_frame() -> bool:
    return bool(_end_frame())


def wait_for_gpu() -> bool:
    if _wait_for_gpu is None:
        return True
    return bool(_wait_for_gpu())


def get_frame_index() -> int:
    return _get_frame_index()


def compile_shader(file_path: str, entry_point: str, profile: str) -> int:
    if not os.path.isfile(file_path):
        raise FileNotFoundError(f"Shader file not found: {file_path}")

    file_path_w = ctypes.create_unicode_buffer(file_path)
    entry = ctypes.create_string_buffer(entry_point.encode('utf-8'))
    prof = ctypes.create_string_buffer(profile.encode('utf-8'))

    out_blob = ctypes.c_void_p()
    out_blob_ptr = ctypes.pointer(out_blob)

    hr = _compile_shader(
        ctypes.cast(file_path_w, ctypes.c_void_p),
        ctypes.cast(entry, ctypes.c_void_p),
        ctypes.cast(prof, ctypes.c_void_p),
        out_blob_ptr
    )

    if hr != 0:
        raise RuntimeError(f"compile_shader failed with code {hr}")

    return out_blob.value or 0


def create_graphics_ps(device: ctypes.c_void_p, vs_blob: ctypes.c_void_p, ps_blob: ctypes.c_void_p) -> int:
    pso = _create_graphics_ps(_to_cvoid(device), _to_cvoid(vs_blob), _to_cvoid(ps_blob))
    if not pso:
        raise RuntimeError("create_graphics_ps returned NULL")
    return pso if isinstance(pso, int) else pso.value


def set_graphics_pipeline(pso: ctypes.c_void_p) -> bool:
    return bool(_set_graphics_pipeline(_to_cvoid(pso)))


def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    usage_bytes = ctypes.create_string_buffer(usage.encode('utf-8'))
    result = _create_buffer(
        _to_cvoid(device),
        ctypes.c_size_t(size),
        ctypes.cast(usage_bytes, ctypes.c_void_p)
    )
    # result - это int (указатель)
    return _get_cached_void_p(result)


def update_subresource(buffer: ctypes.c_void_p, data: bytes) -> bool:
    if not data:
        return False

    # Создаём копию данных
    data_buffer = ctypes.create_string_buffer(data, len(data))
    data_ptr = ctypes.addressof(data_buffer)

    with _TEMP_BUFFERS_LOCK:
        _TEMP_BUFFERS.append(data_buffer)

    try:
        result = _update_subresource(
            _to_cvoid(buffer),
            ctypes.c_void_p(data_ptr),
            ctypes.c_size_t(len(data))
        )
        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_subresource exception: {e}")
        return False

def create_texture_from_memory(
        device: ctypes.c_void_p,
        data: Optional[bytes],
        w: int,
        h: int,
        fmt: str = "rgba8",
) -> ctypes.c_void_p:
    fmt_bytes = ctypes.create_string_buffer(fmt.encode('utf-8'))

    data_ptr = 0
    data_buffer = None

    if data:
        data_buffer = ctypes.create_string_buffer(data, len(data))
        data_ptr = ctypes.addressof(data_buffer)
        with _TEMP_BUFFERS_LOCK:
            _TEMP_BUFFERS.append(data_buffer)

    tex = _create_texture_from_memory(
        _to_cvoid(device),
        ctypes.c_void_p(data_ptr),
        ctypes.c_uint32(w),
        ctypes.c_uint32(h),
        ctypes.cast(fmt_bytes, ctypes.c_void_p),
    )

    return _to_cvoid(tex)


def update_texture(texture: ctypes.c_void_p, data: bytes, w: int, h: int) -> bool:
    if not data:
        return False

    data_buffer = ctypes.create_string_buffer(data, len(data))
    data_ptr = ctypes.addressof(data_buffer)
    with _TEMP_BUFFERS_LOCK:
        _TEMP_BUFFERS.append(data_buffer)

    return bool(_update_texture(
        _to_cvoid(texture),
        ctypes.c_void_p(data_ptr),
        ctypes.c_uint32(w),
        ctypes.c_uint32(h)
    ))


def create_descriptor_heap(
        device: ctypes.c_void_p,
        num_descriptors: int,
        heap_type: int,
        shader_visible: bool = False,
) -> ctypes.c_void_p:
    raw_result = _create_descriptor_heap(
        _to_cvoid(device),
        ctypes.c_uint32(num_descriptors),
        ctypes.c_uint32(heap_type),
        ctypes.c_bool(shader_visible),
    )
    return ctypes.c_void_p(raw_result)


def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    return _GetCPUDescriptorHandleForHeapStart(_to_cvoid(heap))


def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    return _GetGPUDescriptorHandleForHeapStart(_to_cvoid(heap))


def offset_descriptor_handle(base: int, index: int) -> int:
    return _offset_descriptor_handle(ctypes.c_uint64(base), ctypes.c_uint32(index))


def create_shader_resource_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int,
) -> bool:
    return bool(_create_shader_resource_view(
        _to_cvoid(device),
        _to_cvoid(resource),
        ctypes.c_uint64(cpu_handle)
    ))


def create_render_target_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int,
) -> bool:
    return bool(_create_render_target_view(
        _to_cvoid(device),
        _to_cvoid(resource),
        ctypes.c_uint64(cpu_handle)
    ))


def create_constant_buffer_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int,
) -> bool:
    return bool(_create_constant_buffer_view(
        _to_cvoid(device),
        _to_cvoid(resource),
        ctypes.c_uint64(cpu_handle)
    ))


def set_root_descriptor_table(root_index: int, gpu_handle: int) -> bool:
    return bool(_set_root_descriptor_table(ctypes.c_uint32(root_index), ctypes.c_uint64(gpu_handle)))


def set_descriptor_heaps(heaps: Sequence[Any]) -> bool:
    if _set_descriptor_heaps is None:
        raise RuntimeError("set_descriptor_heaps not available")

    ptrs = []
    for h in heaps:
        if h is None:
            continue
        if isinstance(h, ctypes.c_void_p):
            if h.value:
                ptrs.append(h)
        elif hasattr(h, 'value'):
            if h.value:
                ptrs.append(ctypes.c_void_p(h.value))
        elif isinstance(h, int):
            if h:
                ptrs.append(ctypes.c_void_p(h))
        else:
            try:
                val = int(h)
                if val:
                    ptrs.append(ctypes.c_void_p(val))
            except:
                pass

    if not ptrs:
        logger.error("[d3d12_wrapper] No valid heaps to set")
        return False

    try:
        arr = (ctypes.c_void_p * len(ptrs))(*ptrs)
        logger.info(f"[d3d12_wrapper] Setting {len(ptrs)} heaps")
        result = _set_descriptor_heaps(ctypes.c_size_t(len(ptrs)), arr)
        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_descriptor_heaps failed: {e}")
        return False

def set_render_target(rtv: int) -> bool:
    if _set_render_target is not None:
        return bool(_set_render_target(ctypes.c_uint64(rtv)))
    return set_render_targets([rtv])


def set_render_targets(rtvs: Sequence[int]) -> bool:
    if not rtvs:
        return False

    valid_rtvs = [r for r in rtvs if r != 0]
    if not valid_rtvs:
        return False

    count = len(valid_rtvs)
    arr_type = ctypes.c_uint64 * count
    handles = [ctypes.c_uint64(r) for r in valid_rtvs]
    return bool(_set_render_targets(ctypes.c_size_t(count), arr_type(*handles)))


def clear_render_target(rtv: int, color: Tuple[float, float, float, float]) -> bool:
    rgba = (ctypes.c_float * 4)(*color)
    return bool(_clear_render_target(ctypes.c_uint64(rtv), rgba))


def set_viewport(
        x: int,
        y: int,
        w: int,
        h: int,
        min_depth: float = 0.0,
        max_depth: float = 1.0,
) -> bool:
    return bool(_set_viewport(
        ctypes.c_int32(x),
        ctypes.c_int32(y),
        ctypes.c_int32(w),
        ctypes.c_int32(h),
        ctypes.c_float(min_depth),
        ctypes.c_float(max_depth)
    ))


def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> bool:
    return bool(_set_scissor_rect(
        ctypes.c_int32(left),
        ctypes.c_int32(top),
        ctypes.c_int32(right),
        ctypes.c_int32(bottom)
    ))


def set_vertex_buffers(vertex_buffer: ctypes.c_void_p, index_buffer: Optional[ctypes.c_void_p] = None) -> bool:
    if _set_vertex_buffers is None:
        raise RuntimeError("set_vertex_buffers not available")

    try:
        # Получаем значение указателя как int
        vb_val = vertex_buffer.value if hasattr(vertex_buffer, 'value') else int(vertex_buffer)

        # Создаём ctypes.c_void_p с правильным значением
        vb_ptr = ctypes.c_void_p(vb_val)
        ib_ptr = ctypes.c_void_p(0)

        if index_buffer:
            ib_val = index_buffer.value if hasattr(index_buffer, 'value') else int(index_buffer)
            ib_ptr = ctypes.c_void_p(ib_val)

        logger.debug(f"[d3d12_wrapper] set_vertex_buffers: vb=0x{vb_val:X}, ib=0x{ib_val if index_buffer else 0:X}")

        # Передаём указатели напрямую
        result = _set_vertex_buffers(vb_ptr, ib_ptr)
        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_vertex_buffers failed: {e}")
        return False

def draw_instanced(
        vertex_count: int,
        instance_count: int = 1,
        start_vertex: int = 0,
        start_instance: int = 0,
) -> bool:
    return bool(_draw_instanced(
        ctypes.c_uint32(vertex_count),
        ctypes.c_uint32(instance_count),
        ctypes.c_uint32(start_vertex),
        ctypes.c_uint32(start_instance)
    ))


def draw_indexed_instanced(
        index_count: int,
        instance_count: int = 1,
        start_index: int = 0,
        base_vertex: int = 0,
        start_instance: int = 0,
) -> bool:
    return bool(_draw_indexed_instanced(
        ctypes.c_uint32(index_count),
        ctypes.c_uint32(instance_count),
        ctypes.c_uint32(start_index),
        ctypes.c_int32(base_vertex),
        ctypes.c_uint32(start_instance)
    ))


def release_resource(resource: Any) -> None:
    if _release_resource and resource:
        try:
            _release_resource(_to_cvoid(resource))
        except Exception:
            pass


def force_cleanup() -> None:
    if _force_cleanup:
        try:
            _force_cleanup()
        except Exception:
            pass


def get_rtv_descriptor_size() -> int:
    return _get_rtv_descriptor_size()


def get_dsv_descriptor_size() -> int:
    return _get_dsv_descriptor_size()


def set_vsync(enable: bool) -> None:
    if _set_vsync:
        _set_vsync(ctypes.c_bool(enable))


__all__ = [
    "SWAP_CHAIN_BUFFER_COUNT",
    "DXGI_FORMAT_R8G8B8A8_UNORM",
    "create_device",
    "create_command_queue",
    "create_swap_chain",
    "swap_chain_get_buffer",
    "resize_swap_chain",
    "present_swap_chain",
    "begin_frame",
    "end_frame",
    "wait_for_gpu",
    "get_frame_index",
    "force_cleanup",
    "compile_shader",
    "create_graphics_ps",
    "set_graphics_pipeline",
    "create_buffer",
    "update_subresource",
    "release_resource",
    "create_texture_from_memory",
    "update_texture",
    "create_descriptor_heap",
    "GetCPUDescriptorHandleForHeapStart",
    "GetGPUDescriptorHandleForHeapStart",
    "offset_descriptor_handle",
    "set_descriptor_heaps",
    "create_shader_resource_view",
    "create_render_target_view",
    "create_constant_buffer_view",
    "set_root_descriptor_table",
    "set_render_target",
    "set_render_targets",
    "clear_render_target",
    "set_viewport",
    "set_scissor_rect",
    "set_vertex_buffers",
    "draw_instanced",
    "draw_indexed_instanced",
    "get_rtv_descriptor_size",
    "get_dsv_descriptor_size",
    "set_vsync",
]