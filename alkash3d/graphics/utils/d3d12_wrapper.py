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
# Compatibility: ctypes does **not** have c_uintptr in the standard lib.
# ----------------------------------------------------------------------
if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

# ----------------------------------------------------------------------
# Debug flag
# ----------------------------------------------------------------------
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

# ----------------------------------------------------------------------
# ГЛОБАЛЬНОЕ ХРАНИЛИЩЕ БУФЕРОВ
# ----------------------------------------------------------------------
_TEMP_BUFFERS = []
_TEMP_BUFFERS_LOCK = threading.RLock()


def _cleanup_temp_buffers():
    with _TEMP_BUFFERS_LOCK:
        _TEMP_BUFFERS.clear()


atexit.register(_cleanup_temp_buffers)


# ----------------------------------------------------------------------
# Helpers to locate / load the native library
# ----------------------------------------------------------------------
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
        logger.debug(f"[d3d12_wrapper] Loaded function '{name}'")
        return fn
    except AttributeError:
        if required:
            raise RuntimeError(f"Required function '{name}' missing")
        logger.debug(f"[d3d12_wrapper] Function '{name}' not exported")
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


# ----------------------------------------------------------------------
# Helper: convert anything to a ctypes.c_void_p
# ----------------------------------------------------------------------
def _to_cvoid(ptr: Any) -> ctypes.c_void_p:
    if ptr is None:
        return ctypes.c_void_p()
    if isinstance(ptr, ctypes.c_void_p):
        return ptr
    try:
        return ctypes.c_void_p(int(ptr))
    except Exception:
        return ctypes.c_void_p()


# ----------------------------------------------------------------------
# Public API
# ----------------------------------------------------------------------

def create_device() -> ctypes.c_void_p:
    if _create_device is None:
        raise RuntimeError("create_device not available")
    try:
        return _to_cvoid(_create_device())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_device failed: {e}")
        raise


def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    if _create_command_queue is None:
        raise RuntimeError("create_command_queue not available")
    try:
        return _to_cvoid(_create_command_queue(_to_cvoid(device)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_command_queue failed: {e}")
        raise


def create_swap_chain(queue: ctypes.c_void_p, hwnd: int, width: int, height: int) -> ctypes.c_void_p:
    if _create_swap_chain is None:
        raise RuntimeError("create_swap_chain not available")
    try:
        return _to_cvoid(
            _create_swap_chain(
                _to_cvoid(queue),
                ctypes.c_uint64(hwnd),
                ctypes.c_uint32(width),
                ctypes.c_uint32(height),
            )
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_swap_chain failed: {e}")
        raise


def swap_chain_get_buffer(swap: ctypes.c_void_p, idx: int) -> ctypes.c_void_p:
    if _swap_chain_get_buffer is None:
        raise RuntimeError("swap_chain_get_buffer not available")
    try:
        return _to_cvoid(_swap_chain_get_buffer(_to_cvoid(swap), ctypes.c_uint32(idx)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] swap_chain_get_buffer failed: {e}")
        raise


def resize_swap_chain(swap: ctypes.c_void_p, w: int, h: int) -> bool:
    if _resize_swap_chain is None:
        raise RuntimeError("resize_swap_chain not available")
    try:
        return bool(_resize_swap_chain(_to_cvoid(swap), ctypes.c_uint32(w), ctypes.c_uint32(h)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] resize_swap_chain failed: {e}")
        raise


def present_swap_chain(swap: ctypes.c_void_p, sync_interval: int = 1) -> bool:
    if _present_swap_chain is None:
        raise RuntimeError("present_swap_chain not available")
    try:
        return bool(_present_swap_chain(_to_cvoid(swap), ctypes.c_uint32(sync_interval)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] present_swap_chain failed: {e}")
        raise


def begin_frame() -> bool:
    if _begin_frame is None:
        raise RuntimeError("begin_frame not available")
    try:
        return bool(_begin_frame())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] begin_frame failed: {e}")
        raise


def end_frame() -> bool:
    if _end_frame is None:
        raise RuntimeError("end_frame not available")
    try:
        return bool(_end_frame())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] end_frame failed: {e}")
        raise


def wait_for_gpu() -> bool:
    if _wait_for_gpu is None:
        return True
    try:
        return bool(_wait_for_gpu())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] wait_for_gpu failed: {e}")
        return False


def get_frame_index() -> int:
    if _get_frame_index is None:
        raise RuntimeError("get_frame_index not available")
    try:
        return _get_frame_index()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_frame_index failed: {e}")
        raise


def compile_shader(file_path: str, entry_point: str, profile: str) -> int:
    if _compile_shader is None:
        raise RuntimeError("compile_shader not available")

    if not os.path.isfile(file_path):
        raise FileNotFoundError(f"Shader file not found: {file_path}")

    try:
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
    except Exception as e:
        logger.error(f"[d3d12_wrapper] compile_shader exception: {e}")
        raise


def create_graphics_ps(device: ctypes.c_void_p, vs_blob: ctypes.c_void_p, ps_blob: ctypes.c_void_p) -> int:
    if _create_graphics_ps is None:
        raise RuntimeError("create_graphics_ps not available")
    try:
        pso = _create_graphics_ps(_to_cvoid(device), _to_cvoid(vs_blob), _to_cvoid(ps_blob))
        if pso and getattr(pso, "value", 0):
            return pso.value
        if isinstance(pso, int) and pso:
            return pso
        raise RuntimeError("create_graphics_ps returned invalid pointer")
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_graphics_ps exception: {e}")
        raise


def set_graphics_pipeline(pso: ctypes.c_void_p) -> bool:
    if _set_graphics_pipeline is None:
        raise RuntimeError("set_graphics_pipeline not available")
    try:
        return bool(_set_graphics_pipeline(_to_cvoid(pso)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_graphics_pipeline failed: {e}")
        raise


def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    if _create_buffer is None:
        raise RuntimeError("create_buffer not available")
    try:
        usage_bytes = ctypes.create_string_buffer(usage.encode('utf-8'))
        return _to_cvoid(_create_buffer(
            _to_cvoid(device),
            ctypes.c_size_t(size),
            ctypes.cast(usage_bytes, ctypes.c_void_p)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_buffer failed: {e}")
        raise


def update_subresource(buffer: ctypes.c_void_p, data: bytes) -> bool:
    if _update_subresource is None:
        raise RuntimeError("update_subresource not available")

    if not data:
        raise ValueError("update_subresource: data is empty")

    try:
        buffer_ptr = buffer.value if isinstance(buffer, ctypes.c_void_p) else int(buffer)
        if buffer_ptr == 0:
            raise ValueError("update_subresource: buffer is NULL")

        data_buffer = ctypes.create_string_buffer(data, len(data))
        data_ptr = ctypes.addressof(data_buffer)

        with _TEMP_BUFFERS_LOCK:
            _TEMP_BUFFERS.append(data_buffer)

        result = _update_subresource(
            ctypes.c_void_p(buffer_ptr),
            ctypes.c_void_p(data_ptr),
            ctypes.c_size_t(len(data))
        )

        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_subresource failed: {e}")
        raise


def create_texture_from_memory(
        device: ctypes.c_void_p,
        data: Optional[bytes],
        w: int,
        h: int,
        fmt: str = "rgba8",
) -> ctypes.c_void_p:
    if _create_texture_from_memory is None:
        raise RuntimeError("create_texture_from_memory not available")

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

    if not tex or (hasattr(tex, 'value') and tex.value == 0):
        raise RuntimeError("create_texture_from_memory returned NULL")

    return _to_cvoid(tex)


def update_texture(texture: ctypes.c_void_p, data: bytes, w: int, h: int) -> bool:
    if _update_texture is None:
        raise RuntimeError("update_texture not available")

    if not data:
        raise ValueError("update_texture: data is empty")

    try:
        data_buffer = ctypes.create_string_buffer(data, len(data))
        data_ptr = ctypes.addressof(data_buffer)
        with _TEMP_BUFFERS_LOCK:
            _TEMP_BUFFERS.append(data_buffer)

        result = _update_texture(
            _to_cvoid(texture),
            ctypes.c_void_p(data_ptr),
            ctypes.c_uint32(w),
            ctypes.c_uint32(h)
        )

        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_texture failed: {e}")
        raise


def create_descriptor_heap(
        device: ctypes.c_void_p,
        num_descriptors: int,
        heap_type: int,
        shader_visible: bool = False,
) -> ctypes.c_void_p:
    if _create_descriptor_heap is None:
        raise RuntimeError("create_descriptor_heap not available")

    try:
        raw_result = _create_descriptor_heap(
            _to_cvoid(device),
            ctypes.c_uint32(num_descriptors),
            ctypes.c_uint32(heap_type),
            ctypes.c_bool(shader_visible),
        )

        if not raw_result:
            raise RuntimeError("create_descriptor_heap returned NULL")

        return ctypes.c_void_p(raw_result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_descriptor_heap failed: {e}")
        raise


def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if _GetCPUDescriptorHandleForHeapStart is None:
        raise RuntimeError("GetCPUDescriptorHandleForHeapStart not available")
    try:
        if heap is None or (hasattr(heap, 'value') and heap.value == 0):
            raise ValueError("GetCPUDescriptorHandleForHeapStart: heap is NULL")
        return _GetCPUDescriptorHandleForHeapStart(_to_cvoid(heap))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] GetCPUDescriptorHandleForHeapStart failed: {e}")
        raise

def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if _GetGPUDescriptorHandleForHeapStart is None:
        raise RuntimeError("GetGPUDescriptorHandleForHeapStart not available")

    try:
        if heap is None or (hasattr(heap, 'value') and heap.value == 0):
            raise ValueError("GetGPUDescriptorHandleForHeapStart: heap is NULL")

        result = _GetGPUDescriptorHandleForHeapStart(_to_cvoid(heap))

        if result == 0:
            raise RuntimeError("GetGPUDescriptorHandleForHeapStart returned 0")

        return result
    except Exception as e:
        logger.error(f"[d3d12_wrapper] GetGPUDescriptorHandleForHeapStart failed: {e}")
        raise


def offset_descriptor_handle(base: int, index: int) -> int:
    if _offset_descriptor_handle is None:
        raise RuntimeError("offset_descriptor_handle not available")
    try:
        return _offset_descriptor_handle(ctypes.c_uint64(base), ctypes.c_uint32(index))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] offset_descriptor_handle failed: {e}")
        raise


def create_shader_resource_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int,
) -> bool:
    if _create_shader_resource_view is None:
        raise RuntimeError("create_shader_resource_view not available")

    if cpu_handle == 0:
        raise ValueError("create_shader_resource_view: cpu_handle is 0")

    try:
        return bool(_create_shader_resource_view(
            _to_cvoid(device),
            _to_cvoid(resource),
            ctypes.c_uint64(cpu_handle)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_shader_resource_view failed: {e}")
        raise


def create_render_target_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int,
) -> bool:
    if _create_render_target_view is None:
        raise RuntimeError("create_render_target_view not available")

    if cpu_handle == 0:
        raise ValueError("create_render_target_view: cpu_handle is 0")

    try:
        return bool(_create_render_target_view(
            _to_cvoid(device),
            _to_cvoid(resource),
            ctypes.c_uint64(cpu_handle)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_render_target_view failed: {e}")
        raise


def create_constant_buffer_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int,
) -> bool:
    if _create_constant_buffer_view is None:
        raise RuntimeError("create_constant_buffer_view not available")

    if cpu_handle == 0:
        raise ValueError("create_constant_buffer_view: cpu_handle is 0")

    try:
        result = _create_constant_buffer_view(
            _to_cvoid(device),
            _to_cvoid(resource),
            ctypes.c_uint64(cpu_handle)
        )
        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_constant_buffer_view failed: {e}")
        raise


def set_root_descriptor_table(root_index: int, gpu_handle: int) -> bool:
    if _set_root_descriptor_table is None:
        raise RuntimeError("set_root_descriptor_table not available")

    if gpu_handle == 0:
        raise ValueError(f"set_root_descriptor_table: GPU handle is 0")

    try:
        # Убираем все проверки на битые handle
        # Просто передаём handle как есть
        result = _set_root_descriptor_table(ctypes.c_uint32(root_index), ctypes.c_uint64(gpu_handle))
        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_root_descriptor_table failed: {e}")
        raise

def set_descriptor_heaps(heaps: Sequence[Any]) -> bool:
    if _set_descriptor_heaps is None:
        raise RuntimeError("set_descriptor_heaps not available")

    ptrs = []
    for h in heaps:
        if h is None:
            continue

        # h может быть ctypes.c_void_p указателем на кучу
        if isinstance(h, ctypes.c_void_p):
            if h.value != 0:
                ptrs.append(h)
        elif hasattr(h, 'value'):
            # Это может быть ctypes.c_void_p или другой объект с value
            if h.value != 0:
                ptrs.append(ctypes.c_void_p(h.value))
        elif isinstance(h, int):
            if h != 0:
                ptrs.append(ctypes.c_void_p(h))
        else:
            try:
                val = int(h)
                if val != 0:
                    ptrs.append(ctypes.c_void_p(val))
            except:
                pass

    if not ptrs:
        raise ValueError("No valid descriptor heaps to set")

    try:
        arr = (ctypes.c_void_p * len(ptrs))(*ptrs)
        logger.debug(f"[d3d12_wrapper] Setting {len(ptrs)} descriptor heaps")
        result = _set_descriptor_heaps(ctypes.c_size_t(len(ptrs)), arr)
        return bool(result)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_descriptor_heaps failed: {e}")
        raise


def set_render_target(rtv: int) -> bool:
    if rtv == 0:
        raise ValueError("set_render_target: rtv is 0")

    if _set_render_target is not None:
        try:
            return bool(_set_render_target(ctypes.c_uint64(rtv)))
        except Exception as e:
            logger.error(f"[d3d12_wrapper] set_render_target failed: {e}")
            raise
    return set_render_targets([rtv])


def set_render_targets(rtvs: Sequence[int]) -> bool:
    if _set_render_targets is None:
        raise RuntimeError("set_render_targets not available")

    if not rtvs:
        raise ValueError("set_render_targets: no RTVs provided")

    valid_rtvs = [r for r in rtvs if r != 0]
    if not valid_rtvs:
        raise ValueError("set_render_targets: no valid RTVs")

    try:
        count = len(valid_rtvs)
        arr_type = ctypes.c_uint64 * count
        handles = [ctypes.c_uint64(r) for r in valid_rtvs]
        return bool(_set_render_targets(ctypes.c_size_t(count), arr_type(*handles)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_render_targets failed: {e}")
        raise


def clear_render_target(rtv: int, color: Tuple[float, float, float, float]) -> bool:
    if _clear_render_target is None:
        raise RuntimeError("clear_render_target not available")

    if rtv == 0:
        raise ValueError("clear_render_target: rtv is 0")

    try:
        rgba = (ctypes.c_float * 4)(*color)
        return bool(_clear_render_target(ctypes.c_uint64(rtv), rgba))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] clear_render_target failed: {e}")
        raise


def set_viewport(
        x: int,
        y: int,
        w: int,
        h: int,
        min_depth: float = 0.0,
        max_depth: float = 1.0,
) -> bool:
    if _set_viewport is None:
        raise RuntimeError("set_viewport not available")
    try:
        return bool(_set_viewport(
            ctypes.c_int32(x),
            ctypes.c_int32(y),
            ctypes.c_int32(w),
            ctypes.c_int32(h),
            ctypes.c_float(min_depth),
            ctypes.c_float(max_depth)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_viewport failed: {e}")
        raise


def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> bool:
    if _set_scissor_rect is None:
        raise RuntimeError("set_scissor_rect not available")
    try:
        return bool(_set_scissor_rect(
            ctypes.c_int32(left),
            ctypes.c_int32(top),
            ctypes.c_int32(right),
            ctypes.c_int32(bottom)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_scissor_rect failed: {e}")
        raise


def set_vertex_buffers(vertex_buffer: ctypes.c_void_p, index_buffer: Optional[ctypes.c_void_p] = None) -> bool:
    if _set_vertex_buffers is None:
        raise RuntimeError("set_vertex_buffers not available")
    try:
        ib_ptr = index_buffer if index_buffer is not None else ctypes.c_void_p()
        return bool(_set_vertex_buffers(_to_cvoid(vertex_buffer), ib_ptr))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_vertex_buffers failed: {e}")
        raise


def draw_instanced(
        vertex_count: int,
        instance_count: int = 1,
        start_vertex: int = 0,
        start_instance: int = 0,
) -> bool:
    if _draw_instanced is None:
        raise RuntimeError("draw_instanced not available")
    try:
        return bool(_draw_instanced(
            ctypes.c_uint32(vertex_count),
            ctypes.c_uint32(instance_count),
            ctypes.c_uint32(start_vertex),
            ctypes.c_uint32(start_instance)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] draw_instanced failed: {e}")
        raise


def draw_indexed_instanced(
        index_count: int,
        instance_count: int = 1,
        start_index: int = 0,
        base_vertex: int = 0,
        start_instance: int = 0,
) -> bool:
    if _draw_indexed_instanced is None:
        raise RuntimeError("draw_indexed_instanced not available")
    try:
        return bool(_draw_indexed_instanced(
            ctypes.c_uint32(index_count),
            ctypes.c_uint32(instance_count),
            ctypes.c_uint32(start_index),
            ctypes.c_int32(base_vertex),
            ctypes.c_uint32(start_instance)
        ))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] draw_indexed_instanced failed: {e}")
        raise


def release_resource(resource: Any) -> None:
    if _release_resource is None:
        return
    if not resource:
        return
    try:
        _release_resource(_to_cvoid(resource))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] release_resource failed: {e}")


def force_cleanup() -> None:
    if _force_cleanup:
        try:
            _force_cleanup()
        except Exception as e:
            logger.error(f"[d3d12_wrapper] force_cleanup failed: {e}")


def get_rtv_descriptor_size() -> int:
    if _get_rtv_descriptor_size is None:
        raise RuntimeError("get_rtv_descriptor_size not available")
    try:
        return _get_rtv_descriptor_size()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_rtv_descriptor_size failed: {e}")
        raise


def get_dsv_descriptor_size() -> int:
    if _get_dsv_descriptor_size is None:
        raise RuntimeError("get_dsv_descriptor_size not available")
    try:
        return _get_dsv_descriptor_size()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_dsv_descriptor_size failed: {e}")
        raise


def set_vsync(enable: bool) -> None:
    if _set_vsync:
        try:
            _set_vsync(ctypes.c_bool(enable))
        except Exception as e:
            logger.error(f"[d3d12_wrapper] set_vsync failed: {e}")


# ----------------------------------------------------------------------
# Public __all__
# ----------------------------------------------------------------------
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
    "DEBUG",
]