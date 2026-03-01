# alkash3d/graphics/utils/d3d12_wrapper.py
# -*- coding: utf-8 -*-
"""
Thin ctypes wrapper around the native alkash3d_dx12 DLL.
All missing / optional functions fall back to “stub” mode
instead of raising ImportError.
"""

from __future__ import annotations
import ctypes
import os
import sys
from pathlib import Path
from typing import Any, Optional, Sequence, Tuple

from alkash3d.utils import logger

# ----------------------------------------------------------------------
# Compatibility: ctypes does **not** have c_uintptr in the standard lib.
# ----------------------------------------------------------------------
if not hasattr(ctypes, "c_uintptr"):
    # Use a plain pointer‑sized integer type.
    ctypes.c_uintptr = ctypes.c_void_p  # type: ignore[attr-defined]

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
# Helpers to locate / load the native library
# ----------------------------------------------------------------------
def _locate_dll() -> Optional[str]:
    """Searches for the DLL in a few standard locations."""
    global _lib_path
    if _lib_path:
        return _lib_path

    base_dir = Path(__file__).parent
    candidates = [
        base_dir / _DEFAULT_DLL_NAME,
        base_dir.parent.parent / _DEFAULT_DLL_NAME,   # project root
        Path.cwd() / _DEFAULT_DLL_NAME,
        Path(_DEFAULT_DLL_NAME),
    ]

    for cand in candidates:
        if cand.exists():
            _lib_path = str(cand.absolute())
            logger.debug(f"[d3d12_wrapper] Found DLL at: {_lib_path}")
            return _lib_path

    logger.warning("[d3d12_wrapper] DLL not found")
    return None


def _load_lib() -> Optional[ctypes.CDLL]:
    """Loads the native library (once)."""
    global _lib
    if _lib is not None:
        return _lib

    libpath = _locate_dll()
    if libpath is None:
        logger.warning("[d3d12_wrapper] Native DLL missing – stub mode")
        return None

    try:
        _lib = ctypes.CDLL(libpath)
        logger.info(f"[d3d12_wrapper] Loaded native library: {libpath}")
        return _lib
    except Exception as e:
        logger.error(f"[d3d12_wrapper] Failed to load {libpath}: {e}")
        _lib = None
        return None


def _load_func(
    name: str,
    restype,
    argtypes,
    required: bool = False,
):
    """Loads a single function from the DLL and sets its signature."""
    lib = _load_lib()
    if lib is None:
        if required:
            raise RuntimeError(f"[d3d12_wrapper] Required function '{name}' not available")
        return None

    try:
        fn = getattr(lib, name)
        fn.restype = restype
        fn.argtypes = argtypes
        logger.debug(f"[d3d12_wrapper] Loaded function '{name}'")
        return fn
    except AttributeError:
        if required:
            raise RuntimeError(f"[d3d12_wrapper] Required function '{name}' missing")
        logger.debug(f"[d3d12_wrapper] Function '{name}' not exported – optional")
        return None


# ----------------------------------------------------------------------
# Load all exported functions (all optional – stub mode if missing)
# ----------------------------------------------------------------------
# Core device functions
_create_device = _load_func("create_device", ctypes.c_void_p, [], required=False)
_create_command_queue = _load_func("create_command_queue", ctypes.c_void_p, [ctypes.c_void_p], required=False)
_release_resource = _load_func("release_resource", None, [ctypes.c_void_p], required=False)
_force_cleanup = _load_func("force_cleanup", None, [], required=False)

# Swap‑chain
_create_swap_chain = _load_func(
    "create_swap_chain",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uintptr, ctypes.c_uint, ctypes.c_uint],
    required=False,
)
_swap_chain_get_buffer = _load_func(
    "swap_chain_get_buffer",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint],
    required=False,
)
_resize_swap_chain = _load_func(
    "resize_swap_chain",
    ctypes.c_bool,
    [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint],
    required=False,
)
_present_swap_chain = _load_func(
    "present_swap_chain",
    ctypes.c_bool,
    [ctypes.c_void_p, ctypes.c_uint],
    required=False,
)

# Frame control
_begin_frame = _load_func("begin_frame", ctypes.c_bool, [], required=False)
_end_frame = _load_func("end_frame", ctypes.c_bool, [], required=False)
_wait_for_gpu = _load_func("wait_for_gpu", ctypes.c_bool, [], required=False)
_get_frame_index = _load_func("get_frame_index", ctypes.c_uint, [], required=False)

# Shaders
_compile_shader = _load_func(
    "compile_shader",
    ctypes.c_int,
    [
        ctypes.c_wchar_p,   # file_path (wide‑char)
        ctypes.c_char_p,    # entry point
        ctypes.c_char_p,    # profile
        ctypes.POINTER(ctypes.c_void_p),  # out_blob
    ],
    required=False,
)
_create_graphics_ps = _load_func(
    "create_graphics_ps", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p], required=False
)
_set_graphics_pipeline = _load_func(
    "set_graphics_pipeline", ctypes.c_bool, [ctypes.c_void_p], required=False
)

# Buffers
_create_buffer = _load_func(
    "create_buffer", ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p], required=False
)
_update_subresource = _load_func(
    "update_subresource", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t], required=False
)

# Textures
_create_texture_from_memory = _load_func(
    "create_texture_from_memory",
    ctypes.c_void_p,
    [
        ctypes.c_void_p,   # device
        ctypes.c_void_p,   # data ptr (or NULL)
        ctypes.c_uint,    # width
        ctypes.c_uint,    # height
        ctypes.c_char_p,  # format string
    ],
    required=False,
)
_update_texture = _load_func(
    "update_texture", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint], required=False
)

# Descriptor heaps
_create_descriptor_heap = _load_func(
    "create_descriptor_heap",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_bool],
    required=False,
)
_GetCPUDescriptorHandleForHeapStart = _load_func(
    "GetCPUDescriptorHandleForHeapStart", ctypes.c_uintptr, [ctypes.c_void_p], required=False
)
_GetGPUDescriptorHandleForHeapStart = _load_func(
    "GetGPUDescriptorHandleForHeapStart", ctypes.c_uintptr, [ctypes.c_void_p], required=False
)
_offset_descriptor_handle = _load_func(
    "offset_descriptor_handle", ctypes.c_uintptr, [ctypes.c_uintptr, ctypes.c_uint], required=False
)

# Views
_create_shader_resource_view = _load_func(
    "create_shader_resource_view", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p], required=False
)
_create_render_target_view = _load_func(
    "create_render_target_view", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p], required=False
)
_create_constant_buffer_view = _load_func(
    "create_constant_buffer_view", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p], required=False
)

# Rendering commands
_set_root_descriptor_table = _load_func(
    "set_root_descriptor_table", ctypes.c_bool, [ctypes.c_uint, ctypes.c_uint64], required=False
)
_set_descriptor_heaps = _load_func(
    "set_descriptor_heaps", ctypes.c_bool, [ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)], required=False
)
_set_render_target = _load_func(
    "set_render_target", ctypes.c_bool, [ctypes.c_uintptr], required=False
)
_set_render_targets = _load_func(
    "set_render_targets", ctypes.c_bool, [ctypes.c_uint, ctypes.POINTER(ctypes.c_uintptr)], required=False
)
_clear_render_target = _load_func(
    "clear_render_target", ctypes.c_bool, [ctypes.c_uintptr, ctypes.POINTER(ctypes.c_float)], required=False
)
_set_viewport = _load_func(
    "set_viewport", ctypes.c_bool, [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_float, ctypes.c_float], required=False
)
_set_scissor_rect = _load_func(
    "set_scissor_rect", ctypes.c_bool, [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int], required=False
)
_set_vertex_buffers = _load_func(
    "set_vertex_buffers", ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p], required=False
)
_draw_instanced = _load_func(
    "draw_instanced", ctypes.c_bool, [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint], required=False
)
_draw_indexed_instanced = _load_func(
    "draw_indexed_instanced", ctypes.c_bool, [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_uint], required=False
)

# Info
_get_rtv_descriptor_size = _load_func("get_rtv_descriptor_size", ctypes.c_uint, [], required=False)
_get_dsv_descriptor_size = _load_func("get_dsv_descriptor_size", ctypes.c_uint, [], required=False)
_set_vsync = _load_func("set_vsync", None, [ctypes.c_bool], required=False)


# ----------------------------------------------------------------------
# Helper: convert anything to a ctypes.c_void_p (nullptr on failure)
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
# Public API (high‑level wrappers)
# ----------------------------------------------------------------------
def create_device() -> ctypes.c_void_p:
    if _create_device is None:
        logger.debug("[d3d12_wrapper] create_device not available")
        return ctypes.c_void_p(0)
    try:
        return _to_cvoid(_create_device())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_device failed: {e}")
        return ctypes.c_void_p(0)


def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    if _create_command_queue is None:
        logger.debug("[d3d12_wrapper] create_command_queue not available")
        return ctypes.c_void_p(0)
    try:
        return _to_cvoid(_create_command_queue(_to_cvoid(device)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_command_queue failed: {e}")
        return ctypes.c_void_p(0)


def create_swap_chain(queue: ctypes.c_void_p, hwnd: int, width: int, height: int) -> ctypes.c_void_p:
    if _create_swap_chain is None:
        logger.debug("[d3d12_wrapper] create_swap_chain not available")
        return ctypes.c_void_p(0)
    try:
        return _to_cvoid(
            _create_swap_chain(
                _to_cvoid(queue),
                ctypes.c_uintptr(hwnd),
                ctypes.c_uint(width),
                ctypes.c_uint(height),
            )
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_swap_chain failed: {e}")
        return ctypes.c_void_p(0)


def swap_chain_get_buffer(swap: ctypes.c_void_p, idx: int) -> ctypes.c_void_p:
    if _swap_chain_get_buffer is None:
        logger.debug("[d3d12_wrapper] swap_chain_get_buffer not available")
        return ctypes.c_void_p(0)
    try:
        return _to_cvoid(_swap_chain_get_buffer(_to_cvoid(swap), ctypes.c_uint(idx)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] swap_chain_get_buffer failed: {e}")
        return ctypes.c_void_p(0)


def resize_swap_chain(swap: ctypes.c_void_p, w: int, h: int) -> bool:
    if _resize_swap_chain is None:
        logger.debug("[d3d12_wrapper] resize_swap_chain not available")
        return False
    try:
        return bool(_resize_swap_chain(_to_cvoid(swap), ctypes.c_uint(w), ctypes.c_uint(h)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] resize_swap_chain failed: {e}")
        return False


def present_swap_chain(swap: ctypes.c_void_p, sync_interval: int = 1) -> bool:
    if _present_swap_chain is None:
        logger.debug("[d3d12_wrapper] present_swap_chain not available")
        return False
    try:
        return bool(_present_swap_chain(_to_cvoid(swap), ctypes.c_uint(sync_interval)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] present_swap_chain failed: {e}")
        return False


def begin_frame() -> bool:
    if _begin_frame is None:
        logger.debug("[d3d12_wrapper] begin_frame not available")
        return False
    try:
        return bool(_begin_frame())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] begin_frame failed: {e}")
        return False


def end_frame() -> bool:
    if _end_frame is None:
        logger.debug("[d3d12_wrapper] end_frame not available")
        return False
    try:
        return bool(_end_frame())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] end_frame failed: {e}")
        return False


def wait_for_gpu() -> bool:
    if _wait_for_gpu is None:
        logger.debug("[d3d12_wrapper] wait_for_gpu not available")
        return False
    try:
        return bool(_wait_for_gpu())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] wait_for_gpu failed: {e}")
        return False


def get_frame_index() -> int:
    if _get_frame_index is None:
        return 0
    try:
        return _get_frame_index()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_frame_index failed: {e}")
        return 0


def compile_shader(file_path: str, entry_point: str, profile: str) -> int:
    """
    Returns the **pointer value** (int) to the compiled blob,
    or 0 on failure.
    """
    if _compile_shader is None:
        logger.debug("[d3d12_wrapper] compile_shader not available")
        return 0

    if not os.path.isfile(file_path):
        logger.error(f"[d3d12_wrapper] Shader file not found: {file_path}")
        return 0

    try:
        entry = ctypes.c_char_p(entry_point.encode())
        prof = ctypes.c_char_p(profile.encode())
        out_blob = ctypes.c_void_p()
        hr = _compile_shader(
            ctypes.c_wchar_p(file_path),
            entry,
            prof,
            ctypes.byref(out_blob),
        )
        if hr != 0:
            logger.error(f"[d3d12_wrapper] compile_shader failed with HRESULT 0x{hr:X}")
            return 0
        return out_blob.value or 0
    except Exception as e:
        logger.error(f"[d3d12_wrapper] compile_shader exception: {e}")
        return 0


def create_graphics_ps(device: ctypes.c_void_p, vs_blob: ctypes.c_void_p, ps_blob: ctypes.c_void_p) -> Optional[int]:
    if _create_graphics_ps is None:
        logger.debug("[d3d12_wrapper] create_graphics_ps not available")
        return None
    try:
        pso = _create_graphics_ps(_to_cvoid(device), _to_cvoid(vs_blob), _to_cvoid(ps_blob))
        if pso and getattr(pso, "value", 0):
            return pso.value
        if isinstance(pso, int) and pso:
            return pso
        return None
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_graphics_ps exception: {e}")
        return None


def set_graphics_pipeline(pso: ctypes.c_void_p) -> bool:
    if _set_graphics_pipeline is None:
        logger.debug("[d3d12_wrapper] set_graphics_pipeline not available")
        return False
    try:
        return bool(_set_graphics_pipeline(_to_cvoid(pso)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_graphics_pipeline failed: {e}")
        return False


def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    if _create_buffer is None:
        logger.debug("[d3d12_wrapper] create_buffer not available")
        return ctypes.c_void_p(0)
    try:
        usage_bytes = usage.encode()
        return _to_cvoid(_create_buffer(_to_cvoid(device), ctypes.c_size_t(size), usage_bytes))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_buffer failed: {e}")
        return ctypes.c_void_p(0)


def update_subresource(buffer: ctypes.c_void_p, data: bytes) -> bool:
    if _update_subresource is None:
        logger.debug("[d3d12_wrapper] update_subresource not available")
        return False
    try:
        raw = ctypes.create_string_buffer(data, len(data))
        return bool(_update_subresource(_to_cvoid(buffer), ctypes.addressof(raw), ctypes.c_size_t(len(data))))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_subresource failed: {e}")
        return False


def create_texture_from_memory(
    device: ctypes.c_void_p,
    data: Optional[bytes],
    w: int,
    h: int,
    fmt: str = "rgba8",
) -> ctypes.c_void_p:
    if _create_texture_from_memory is None:
        logger.debug("[d3d12_wrapper] create_texture_from_memory not available")
        return ctypes.c_void_p(0)

    fmt_bytes = fmt.encode()
    if data:
        buf = ctypes.create_string_buffer(data, len(data))
        data_ptr = ctypes.c_void_p(ctypes.addressof(buf))
    else:
        data_ptr = ctypes.c_void_p()

    tex = _create_texture_from_memory(
        _to_cvoid(device),
        data_ptr,
        ctypes.c_uint(w),
        ctypes.c_uint(h),
        ctypes.c_char_p(fmt_bytes),
    )
    return _to_cvoid(tex)


def update_texture(texture: ctypes.c_void_p, data: bytes, w: int, h: int) -> bool:
    if _update_texture is None:
        logger.debug("[d3d12_wrapper] update_texture not available")
        return False
    try:
        buf = ctypes.create_string_buffer(data, len(data))
        return bool(_update_texture(_to_cvoid(texture), ctypes.c_void_p(ctypes.addressof(buf)), ctypes.c_uint(w), ctypes.c_uint(h)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_texture failed: {e}")
        return False


def create_descriptor_heap(
    device: ctypes.c_void_p,
    num_descriptors: int,
    heap_type: int,
    shader_visible: bool = False,
) -> ctypes.c_void_p:
    if _create_descriptor_heap is None:
        logger.debug("[d3d12_wrapper] create_descriptor_heap not available")
        return ctypes.c_void_p(0)
    try:
        return _to_cvoid(
            _create_descriptor_heap(
                _to_cvoid(device),
                ctypes.c_uint(num_descriptors),
                ctypes.c_uint(heap_type),
                ctypes.c_bool(shader_visible),
            )
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_descriptor_heap failed: {e}")
        return ctypes.c_void_p(0)


def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if _GetCPUDescriptorHandleForHeapStart is None:
        return 0
    try:
        return _GetCPUDescriptorHandleForHeapStart(_to_cvoid(heap))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] GetCPUDescriptorHandleForHeapStart failed: {e}")
        return 0


def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if _GetGPUDescriptorHandleForHeapStart is None:
        return 0
    try:
        return _GetGPUDescriptorHandleForHeapStart(_to_cvoid(heap))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] GetGPUDescriptorHandleForHeapStart failed: {e}")
        return 0


def offset_descriptor_handle(base: int, index: int) -> int:
    if _offset_descriptor_handle is None:
        # fallback – 32‑byte stride (typical for D3D12)
        return base + index * 32
    try:
        return _offset_descriptor_handle(ctypes.c_uintptr(base), ctypes.c_uint(index))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] offset_descriptor_handle failed: {e}")
        return base + index * 32


def create_shader_resource_view(
    device: ctypes.c_void_p,
    resource: ctypes.c_void_p,
    cpu_handle: int,
) -> bool:
    if _create_shader_resource_view is None:
        logger.debug("[d3d12_wrapper] create_shader_resource_view not available")
        return False
    try:
        return bool(_create_shader_resource_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_shader_resource_view failed: {e}")
        return False


def create_render_target_view(
    device: ctypes.c_void_p,
    resource: ctypes.c_void_p,
    cpu_handle: int,
) -> bool:
    if _create_render_target_view is None:
        logger.debug("[d3d12_wrapper] create_render_target_view not available")
        return False
    try:
        return bool(_create_render_target_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_render_target_view failed: {e}")
        return False


def create_constant_buffer_view(
    device: ctypes.c_void_p,
    resource: ctypes.c_void_p,
    cpu_handle: int,
) -> bool:
    if _create_constant_buffer_view is None:
        logger.debug("[d3d12_wrapper] create_constant_buffer_view not available")
        return False
    try:
        return bool(_create_constant_buffer_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_constant_buffer_view failed: {e}")
        return False


def set_root_descriptor_table(root_index: int, gpu_handle: int) -> bool:
    if _set_root_descriptor_table is None:
        logger.debug("[d3d12_wrapper] set_root_descriptor_table not available")
        return False
    try:
        return bool(_set_root_descriptor_table(ctypes.c_uint(root_index), ctypes.c_uint64(gpu_handle)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_root_descriptor_table failed: {e}")
        return False


def set_descriptor_heaps(heaps: Sequence[Any]) -> bool:
    if _set_descriptor_heaps is None:
        logger.debug("[d3d12_wrapper] set_descriptor_heaps not available")
        return False

    ptrs = []
    for h in heaps:
        if h is None:
            continue
        if isinstance(h, ctypes.c_void_p):
            val = h.value or 0
        elif isinstance(h, int):
            val = h
        elif hasattr(h, "heap_ptr"):
            hp = getattr(h, "heap_ptr")
            val = hp.value if isinstance(hp, ctypes.c_void_p) else int(hp)
        elif hasattr(h, "value"):
            val = h.value or 0
        else:
            continue
        ptrs.append(ctypes.c_void_p(val))

    if not ptrs:
        logger.debug("[d3d12_wrapper] No descriptor heaps to set")
        return False

    try:
        arr = (ctypes.c_void_p * len(ptrs))(*ptrs)
        return bool(_set_descriptor_heaps(ctypes.c_uint(len(ptrs)), arr))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_descriptor_heaps failed: {e}")
        return False


def set_render_target(rtv: int) -> bool:
    if _set_render_target is not None:
        try:
            return bool(_set_render_target(ctypes.c_uintptr(rtv)))
        except Exception as e:
            logger.error(f"[d3d12_wrapper] set_render_target failed: {e}")
            return False
    # fallback – use set_render_targets with single element
    return set_render_targets([rtv])


def set_render_targets(rtvs: Sequence[int]) -> bool:
    if _set_render_targets is None:
        logger.debug("[d3d12_wrapper] set_render_targets not available")
        return False
    if not rtvs:
        return False
    try:
        count = len(rtvs)
        arr_type = ctypes.c_uintptr * count
        return bool(_set_render_targets(ctypes.c_uint(count), arr_type(*[ctypes.c_uintptr(r) for r in rtvs])))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_render_targets failed: {e}")
        return False


def clear_render_target(rtv: int, color: Tuple[float, float, float, float]) -> bool:
    if _clear_render_target is None:
        logger.debug("[d3d12_wrapper] clear_render_target not available")
        return False
    try:
        rgba = (ctypes.c_float * 4)(*color)
        return bool(_clear_render_target(ctypes.c_uintptr(rtv), rgba))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] clear_render_target failed: {e}")
        return False


def set_viewport(
    x: int,
    y: int,
    w: int,
    h: int,
    min_depth: float = 0.0,
    max_depth: float = 1.0,
) -> bool:
    if _set_viewport is None:
        logger.debug("[d3d12_wrapper] set_viewport not available")
        return False
    try:
        return bool(_set_viewport(ctypes.c_int(x), ctypes.c_int(y), ctypes.c_int(w), ctypes.c_int(h), ctypes.c_float(min_depth), ctypes.c_float(max_depth)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_viewport failed: {e}")
        return False


def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> bool:
    if _set_scissor_rect is None:
        logger.debug("[d3d12_wrapper] set_scissor_rect not available")
        return False
    try:
        return bool(_set_scissor_rect(ctypes.c_int(left), ctypes.c_int(top), ctypes.c_int(right), ctypes.c_int(bottom)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_scissor_rect failed: {e}")
        return False


def set_vertex_buffers(vertex_buffer: ctypes.c_void_p, index_buffer: Optional[ctypes.c_void_p] = None) -> bool:
    if _set_vertex_buffers is None:
        logger.debug("[d3d12_wrapper] set_vertex_buffers not available")
        return False
    try:
        ib_ptr = index_buffer if index_buffer is not None else ctypes.c_void_p()
        return bool(_set_vertex_buffers(_to_cvoid(vertex_buffer), ib_ptr))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_vertex_buffers failed: {e}")
        return False


def draw_instanced(
    vertex_count: int,
    instance_count: int = 1,
    start_vertex: int = 0,
    start_instance: int = 0,
) -> bool:
    if _draw_instanced is None:
        logger.debug("[d3d12_wrapper] draw_instanced not available")
        return False
    try:
        return bool(_draw_instanced(ctypes.c_uint(vertex_count), ctypes.c_uint(instance_count), ctypes.c_uint(start_vertex), ctypes.c_uint(start_instance)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] draw_instanced failed: {e}")
        return False


def draw_indexed_instanced(
    index_count: int,
    instance_count: int = 1,
    start_index: int = 0,
    base_vertex: int = 0,
    start_instance: int = 0,
) -> bool:
    if _draw_indexed_instanced is None:
        logger.debug("[d3d12_wrapper] draw_indexed_instanced not available")
        return False
    try:
        return bool(_draw_indexed_instanced(ctypes.c_uint(index_count), ctypes.c_uint(instance_count), ctypes.c_uint(start_index), ctypes.c_int(base_vertex), ctypes.c_uint(start_instance)))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] draw_indexed_instanced failed: {e}")
        return False


def release_resource(resource: Any) -> None:
    if _release_resource is None:
        logger.debug("[d3d12_wrapper] release_resource not available")
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
        return 32
    try:
        return _get_rtv_descriptor_size()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_rtv_descriptor_size failed: {e}")
        return 32


def get_dsv_descriptor_size() -> int:
    if _get_dsv_descriptor_size is None:
        return 8
    try:
        return _get_dsv_descriptor_size()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_dsv_descriptor_size failed: {e}")
        return 8


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
    # Core API
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
    # Shaders
    "compile_shader",
    "create_graphics_ps",
    "set_graphics_pipeline",
    # Buffers
    "create_buffer",
    "update_subresource",
    "release_resource",
    # Textures
    "create_texture_from_memory",
    "update_texture",
    # Descriptor heaps
    "create_descriptor_heap",
    "GetCPUDescriptorHandleForHeapStart",
    "GetGPUDescriptorHandleForHeapStart",
    "offset_descriptor_handle",
    "set_descriptor_heaps",
    # Views
    "create_shader_resource_view",
    "create_render_target_view",
    "create_constant_buffer_view",
    # Rendering commands
    "set_root_descriptor_table",
    "set_render_target",
    "set_render_targets",
    "clear_render_target",
    "set_viewport",
    "set_scissor_rect",
    "set_vertex_buffers",
    "draw_instanced",
    "draw_indexed_instanced",
    # Info
    "get_rtv_descriptor_size",
    "get_dsv_descriptor_size",
    "set_vsync",
    "DEBUG",
]
