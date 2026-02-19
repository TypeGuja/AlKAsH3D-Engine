# alkash3d/graphics/utils/d3d12_wrapper.py
# -*- coding: utf-8 -*-
"""
Thin‑wrapper over the Rust crate ``alkash3d_dx12``.
"""

from __future__ import annotations

import ctypes
import os
import sys
import traceback
from pathlib import Path
from typing import Callable, Any, Tuple, Optional

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print("[D3D12_WRAPPER]", *args, **kwargs)


# ----------------------------------------------------------------------
# Попытка импортировать наш обычный logger (если он уже инициализирован)
# ----------------------------------------------------------------------
try:
    from alkash3d.utils.logger import logger
except Exception:
    logger = None

# ----------------------------------------------------------------------
# Путь к нативной DLL (Windows → .dll, иначе .so)
# ----------------------------------------------------------------------
_ext = ".dll" if sys.platform.startswith("win") else ".so"
_lib_path = Path(__file__).with_name(f"alkash3d_dx12{_ext}")

if not _lib_path.is_file():
    raise RuntimeError(f"[d3d12_wrapper] Native library not found: {_lib_path}")

# ----------------------------------------------------------------------
# Загружаем библиотеку
# ----------------------------------------------------------------------
_lib = ctypes.CDLL(str(_lib_path))

if logger:
    logger.debug(f"[d3d12_wrapper] Loaded library from: {_lib_path}")
else:
    debug_print(f"Loaded library from: {_lib_path}")

# Для совместимости с более старыми версиями Python
if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

# ----------------------------------------------------------------------
# Служебные константы
# ----------------------------------------------------------------------
SWAP_CHAIN_BUFFER_COUNT = 2
DXGI_FORMAT_R8G8B8A8_UNORM = 28


# ----------------------------------------------------------------------
# Вспомогательная функция – безопасно превращаем любой указатель/целое
# в ``ctypes.c_void_p``.
# ----------------------------------------------------------------------
def _to_cvoid(ptr: Any) -> ctypes.c_void_p:
    """Возвращает ``ctypes.c_void_p`` независимо от того, что передано."""
    if isinstance(ptr, ctypes.c_void_p):
        return ptr
    if ptr is None:
        return ctypes.c_void_p()
    # Исправление: если это байтовая строка, преобразуем в int
    if isinstance(ptr, bytes):
        try:
            # Пытаемся интерпретировать как адрес в hex формате
            if ptr.startswith(b'0x'):
                return ctypes.c_void_p(int(ptr, 16))
            # Иначе как целое число
            return ctypes.c_void_p(int.from_bytes(ptr, byteorder='little'))
        except:
            return ctypes.c_void_p()
    return ctypes.c_void_p(int(ptr))


# ----------------------------------------------------------------------
# Универсальная загрузка функции из DLL.
# ----------------------------------------------------------------------
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
        else:
            debug_print(f"Loaded function '{name}'")
        return fn
    except AttributeError as e:
        if required:
            raise RuntimeError(
                f"[d3d12_wrapper] Required function '{name}' not exported from '{_lib_path}'"
            ) from e
        if logger:
            logger.debug(f"[d3d12_wrapper] Function '{name}' not found, skipping")
        else:
            debug_print(f"Function '{name}' not found, skipping")
        return None


# ----------------------------------------------------------------------
# Загрузка всех нужных функций
# ----------------------------------------------------------------------
debug_print("Loading functions from DLL...")

_create_device = _load_func("create_device", ctypes.c_void_p, [], required=True)

_create_command_queue = _load_func(
    "create_command_queue",
    ctypes.c_void_p,
    [ctypes.c_void_p],
    required=True,
)

_create_swap_chain = _load_func(
    "create_swap_chain",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uintptr, ctypes.c_uint, ctypes.c_uint],
    required=True,
)

_swap_chain_get_buffer = _load_func(
    "swap_chain_get_buffer",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint],
    required=True,
)

_resize_swap_chain = _load_func(
    "resize_swap_chain", None, [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint]
)

_present_swap_chain = _load_func(
    "present_swap_chain", None, [ctypes.c_void_p, ctypes.c_uint]
)

_compile_shader = _load_func(
    "compile_shader",
    ctypes.c_int,
    [
        ctypes.c_wchar_p,  # путь к файлу (UTF‑16)
        ctypes.c_char_p,  # entry point
        ctypes.c_char_p,  # профиль
        ctypes.POINTER(ctypes.c_void_p),  # out_blob
    ],
    required=True,
)

_create_graphics_ps = _load_func(
    "create_graphics_ps",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=True,
)

_set_graphics_pipeline = _load_func(
    "set_graphics_pipeline", None, [ctypes.c_void_p], required=True
)

_create_buffer = _load_func(
    "create_buffer",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p],
    required=True,
)

_update_subresource = _load_func(
    "update_subresource", None, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
)

_create_texture_from_memory = _load_func(
    "create_texture_from_memory",
    ctypes.c_void_p,
    [
        ctypes.c_void_p,  # device
        ctypes.c_void_p,  # data (может быть NULL)
        ctypes.c_uint,  # width
        ctypes.c_uint,  # height
        ctypes.c_char_p,  # fmt string (UTF‑8)
        ctypes.c_bool,  # upload?  True → UPLOAD‑heap, False → DEFAULT‑heap
    ],
    required=True,
)

_update_texture = _load_func(
    "update_texture",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint],
)

_create_descriptor_heap = _load_func(
    "create_descriptor_heap",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_bool],
    required=True,
)

_GetCPUDescriptorHandleForHeapStart = _load_func(
    "GetCPUDescriptorHandleForHeapStart",
    ctypes.c_uintptr,
    [ctypes.c_void_p],
    required=True,
)

_GetGPUDescriptorHandleForHeapStart = _load_func(
    "GetGPUDescriptorHandleForHeapStart",
    ctypes.c_uintptr,
    [ctypes.c_void_p],
    required=True,
)

_offset_descriptor_handle = _load_func(
    "offset_descriptor_handle", ctypes.c_uintptr, [ctypes.c_uintptr, ctypes.c_uint], required=True
)

_create_shader_resource_view = _load_func(
    "create_shader_resource_view",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=True,
)

_create_render_target_view = _load_func(
    "create_render_target_view",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=True,
)

_set_root_descriptor_table = _load_func(
    "set_root_descriptor_table", None, [ctypes.c_uint, ctypes.c_uintptr], required=True
)

_set_descriptor_heaps = _load_func(
    "set_descriptor_heaps",
    None,
    [ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)],
    required=True,
)

_set_render_target = _load_func(
    "set_render_target", None, [ctypes.c_uintptr], required=False
)

_set_render_targets = _load_func(
    "set_render_targets",
    None,
    [ctypes.c_size_t, ctypes.POINTER(ctypes.c_uintptr)],
    required=True,
)

_clear_render_target = _load_func(
    "clear_render_target",
    None,
    [ctypes.c_uintptr, ctypes.POINTER(ctypes.c_float)],
)

_set_viewport = _load_func(
    "set_viewport",
    None,
    [
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_float,
        ctypes.c_float,
    ],
    required=True,
)

_set_scissor_rect = _load_func(
    "set_scissor_rect", None, [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int], required=True
)

_set_vertex_buffers = _load_func(
    "set_vertex_buffers", None, [ctypes.c_void_p, ctypes.c_void_p], required=True
)

_draw_instanced = _load_func(
    "draw_instanced",
    None,
    [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint],
    required=True,
)

_draw_indexed_instanced = _load_func(
    "draw_indexed_instanced",
    None,
    [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_uint],
    required=True,
)

_wait_for_gpu = _load_func("wait_for_gpu", None, [], required=True)
_release_resource = _load_func("release_resource", None, [ctypes.c_void_p], required=True)
_get_frame_index = _load_func("get_frame_index", ctypes.c_uint, [], required=True)
_get_rtv_descriptor_size = _load_func("get_rtv_descriptor_size", ctypes.c_uint, [], required=True)
_get_dsv_descriptor_size = _load_func("get_dsv_descriptor_size", ctypes.c_uint, [], required=True)

# optional V‑sync setter (может отсутствовать в DLL)
_set_vsync = _load_func("set_vsync", None, [ctypes.c_bool], required=False)

# ----------------------------------------------------------------------
# NEW: создание CBV‑дескриптора для постоянного буфера
# ----------------------------------------------------------------------
_create_constant_buffer_view = _load_func(
    "create_constant_buffer_view",
    None,
    [
        ctypes.c_void_p,  # device
        ctypes.c_void_p,  # resource (ID3D12Resource)
        ctypes.c_void_p,  # CPU‑descriptor‑handle (умолчание intptr)
    ],
    required=False,  # если DLL без этой функции – fallback‑режим
)


def create_constant_buffer_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: ctypes.c_void_p,
) -> None:
    """Создаёт CBV‑дескриптор для буфера."""
    debug_print(f"create_constant_buffer_view(device={hex(device.value if device else 0)}, "
                f"resource={hex(resource.value if resource else 0)}, "
                f"cpu_handle={hex(cpu_handle.value if cpu_handle else 0)})")
    if device and resource and cpu_handle and _create_constant_buffer_view:
        _create_constant_buffer_view(_to_cvoid(device), _to_cvoid(resource), _to_cvoid(cpu_handle))
    else:
        debug_print("  SKIPPED: missing parameters or function not available")


# ----------------------------------------------------------------------
# High‑level API (более «дружественные» функции)
# ----------------------------------------------------------------------
def create_device() -> ctypes.c_void_p:
    debug_print("create_device() called")
    try:
        result = _create_device()
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    debug_print(f"create_command_queue(device={hex(device.value if device else 0)})")
    if not device:
        debug_print("  -> NULL (no device)")
        return ctypes.c_void_p()
    try:
        result = _create_command_queue(device)
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def create_swap_chain(
        command_queue: ctypes.c_void_p, hwnd: int, width: int, height: int
) -> ctypes.c_void_p:
    debug_print(f"create_swap_chain(queue={hex(command_queue.value if command_queue else 0)}, "
                f"hwnd={hex(hwnd)}, width={width}, height={height})")
    if not command_queue or not hwnd:
        debug_print("  -> NULL (missing parameters)")
        return ctypes.c_void_p()
    try:
        result = _create_swap_chain(
            command_queue,
            ctypes.c_uintptr(hwnd),
            ctypes.c_uint(width),
            ctypes.c_uint(height),
        )
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def resize_swap_chain(swap_chain: ctypes.c_void_p, width: int, height: int) -> None:
    debug_print(f"resize_swap_chain(swap={hex(swap_chain.value if swap_chain else 0)}, "
                f"width={width}, height={height})")
    if swap_chain:
        try:
            _resize_swap_chain(swap_chain, ctypes.c_uint(width), ctypes.c_uint(height))
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  SKIPPED: no swap chain")


def present_swap_chain(swap_chain: ctypes.c_void_p, sync_interval: int = 1) -> None:
    debug_print(f"present_swap_chain(swap={hex(swap_chain.value if swap_chain else 0)}, "
                f"sync={sync_interval})")
    if swap_chain:
        try:
            _present_swap_chain(swap_chain, ctypes.c_uint(sync_interval))
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  SKIPPED: no swap chain")


def compile_shader(file_path: str, entry_point: str, profile: str) -> int:
    debug_print(f"compile_shader(file='{file_path}', entry='{entry_point}', profile='{profile}')")
    if not os.path.isfile(file_path):
        debug_print(f"  ERROR: file not found: {file_path}")
        raise FileNotFoundError(file_path)

    src_utf16 = os.path.abspath(file_path)
    entry_c = ctypes.c_char_p(entry_point.encode("utf-8"))
    profile_c = ctypes.c_char_p(profile.encode("utf-8"))
    out_blob = ctypes.c_void_p()

    try:
        hr = _compile_shader(src_utf16, entry_c, profile_c, ctypes.byref(out_blob))
        debug_print(f"  HRESULT: {hr:#x}, blob: {hex(out_blob.value)}")
        if hr != 0:
            raise RuntimeError(f"Shader compilation failed: HRESULT {hr:#x}")
        return out_blob.value
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        raise


def create_graphics_ps(
        device: ctypes.c_void_p, vs_blob: ctypes.c_void_p, ps_blob: ctypes.c_void_p
) -> ctypes.c_void_p:
    debug_print(f"create_graphics_ps(device={hex(device.value if device else 0)}, "
                f"vs={hex(vs_blob.value if vs_blob else 0)}, "
                f"ps={hex(ps_blob.value if ps_blob else 0)})")
    if not device or not vs_blob or not ps_blob:
        debug_print("  -> NULL (missing parameters)")
        return ctypes.c_void_p()
    try:
        result = _create_graphics_ps(device, vs_blob, ps_blob)
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def set_graphics_pipeline(pso: ctypes.c_void_p) -> None:
    """Установить PSO (один вызов)."""
    debug_print(f"set_graphics_pipeline(pso={hex(pso.value if pso else 0)})")
    if pso:
        try:
            _set_graphics_pipeline(pso)
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  SKIPPED: no PSO")


def swap_chain_get_buffer(swap_chain: ctypes.c_void_p, buffer_index: int) -> ctypes.c_void_p:
    debug_print(f"swap_chain_get_buffer(swap={hex(swap_chain.value if swap_chain else 0)}, "
                f"index={buffer_index})")
    if not swap_chain:
        debug_print("  -> NULL (no swap chain)")
        return ctypes.c_void_p()
    try:
        result = _swap_chain_get_buffer(swap_chain, ctypes.c_uint(buffer_index))
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    debug_print(f"create_buffer(device={hex(device.value if device else 0)}, "
                f"size={size}, usage='{usage}')")
    if not device or size <= 0:
        debug_print("  -> NULL (invalid parameters)")
        return ctypes.c_void_p()
    usage_bytes = usage.encode("utf-8")
    try:
        result = _create_buffer(device, ctypes.c_size_t(size), ctypes.c_char_p(usage_bytes))
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def update_subresource(buffer: Any, data: bytes) -> None:
    debug_print(f"update_subresource(buffer={hex(buffer.value if buffer else 0)}, size={len(data)})")
    if not buffer or not data:
        debug_print("  SKIPPED: missing parameters")
        return
    raw = ctypes.create_string_buffer(data, len(data))
    try:
        _update_subresource(_to_cvoid(buffer), ctypes.c_void_p(ctypes.addressof(raw)), ctypes.c_size_t(len(data)))
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()


def create_texture_from_memory(
        device: ctypes.c_void_p,
        data: Optional[bytes],
        width: int,
        height: int,
        fmt: str | bytes = "rgba8",
) -> ctypes.c_void_p:
    """
    `data` может быть ``None`` – в этом случае создаётся *DEFAULT*‑heap
    (чтобы использовать ресурс как render‑target / depth‑buffer).
    Если `data` присутствует, используем *UPLOAD*‑heap, потому что
    нам нужно выполнить Map/Copy, а DEFAULT‑heap не поддерживает Map.
    """
    debug_print(f"create_texture_from_memory(device={hex(device.value if device else 0)}, "
                f"data={len(data) if data else 'None'}, width={width}, height={height}, fmt='{fmt}')")

    if not device or width <= 0 or height <= 0:
        debug_print("  -> NULL (invalid parameters)")
        return ctypes.c_void_p()

    fmt_bytes = fmt if isinstance(fmt, bytes) else str(fmt).encode("utf-8")

    data_ptr = ctypes.c_void_p()
    if data:
        buf = ctypes.create_string_buffer(data, len(data))
        data_ptr = ctypes.c_void_p(ctypes.addressof(buf))

    # Выбираем тип heap: upload=True ⇔ у нас есть начальные данные.
    upload = bool(data)
    debug_print(f"  upload={upload}")

    try:
        result = _create_texture_from_memory(
            device,
            data_ptr,
            ctypes.c_uint(width),
            ctypes.c_uint(height),
            ctypes.c_char_p(fmt_bytes),
            ctypes.c_bool(upload),
        )
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return ctypes.c_void_p()


def update_texture(texture: ctypes.c_void_p, data: bytes, width: int, height: int) -> None:
    debug_print(f"update_texture(texture={hex(texture.value if texture else 0)}, "
                f"size={len(data)}, width={width}, height={height})")
    if not texture or not data:
        debug_print("  SKIPPED: missing parameters")
        return
    buf = ctypes.create_string_buffer(data, len(data))
    try:
        _update_texture(_to_cvoid(texture), ctypes.c_void_p(ctypes.addressof(buf)), width, height)
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()


def create_descriptor_heap(device: ctypes.c_void_p, num_descriptors: int, heap_type: int,
                           shader_visible: bool) -> ctypes.c_void_p:
    debug_print(f"create_descriptor_heap(device={hex(device.value if device else 0)}, "
                f"num={num_descriptors}, type={heap_type}, shader_visible={shader_visible})")
    if not device or num_descriptors <= 0:
        debug_print("  -> NULL (invalid parameters)")
        raise RuntimeError("Invalid parameters for descriptor heap")
    try:
        result = _create_descriptor_heap(device, ctypes.c_uint(num_descriptors), ctypes.c_uint(heap_type),
                                         ctypes.c_bool(shader_visible))
        debug_print(f"  -> {hex(result)}")
        return _to_cvoid(result)
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        raise


def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    debug_print(f"GetCPUDescriptorHandleForHeapStart(heap={hex(heap.value if heap else 0)})")
    if not heap:
        debug_print("  -> 0 (no heap)")
        return 0
    try:
        result = _GetCPUDescriptorHandleForHeapStart(_to_cvoid(heap))
        debug_print(f"  -> {hex(result)}")
        return result
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return 0


def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    debug_print(f"GetGPUDescriptorHandleForHeapStart(heap={hex(heap.value if heap else 0)})")
    if not heap:
        debug_print("  -> 0 (no heap)")
        return 0
    try:
        result = _GetGPUDescriptorHandleForHeapStart(_to_cvoid(heap))
        debug_print(f"  -> {hex(result)}")
        return result
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return 0


def offset_descriptor_handle(base: int, index: int) -> int:
    debug_print(f"offset_descriptor_handle(base={hex(base)}, index={index})")
    try:
        result = _offset_descriptor_handle(ctypes.c_uintptr(base), ctypes.c_uint(index))
        debug_print(f"  -> {hex(result)}")
        return result
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()
        return base


def create_shader_resource_view(device: ctypes.c_void_p, resource: ctypes.c_void_p, cpu_handle: int) -> None:
    debug_print(f"create_shader_resource_view(device={hex(device.value if device else 0)}, "
                f"resource={hex(resource.value if resource else 0)}, cpu_handle={hex(cpu_handle)})")
    if device and resource and cpu_handle:
        try:
            _create_shader_resource_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle))
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  SKIPPED: missing parameters")


def create_render_target_view(device: ctypes.c_void_p, resource: ctypes.c_void_p, cpu_handle: int) -> None:
    debug_print(f"create_render_target_view(device={hex(device.value if device else 0)}, "
                f"resource={hex(resource.value if resource else 0)}, cpu_handle={hex(cpu_handle)})")
    if device and resource and cpu_handle:
        try:
            _create_render_target_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle))
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  SKIPPED: missing parameters")


def set_root_descriptor_table(root_index: int, gpu_handle: int) -> None:
    debug_print(f"set_root_descriptor_table(root_index={root_index}, gpu_handle={hex(gpu_handle)})")
    try:
        _set_root_descriptor_table(ctypes.c_uint(root_index), ctypes.c_uintptr(gpu_handle))
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def set_descriptor_heaps(heaps: Tuple[ctypes.c_void_p, ...]) -> None:
    """
    ``heaps`` – кортеж/список *raw* дескриптор‑хипов (ctypes.c_void_p).
    Если в список передали объект ``DescriptorHeap`` – берём его атрибут ``heap``.
    """
    debug_print(f"set_descriptor_heaps(count={len(heaps)})")
    raw = []
    for i, h in enumerate(heaps):
        if hasattr(h, "heap"):
            # Если это объект DescriptorHeap, берем его атрибут heap
            heap_ptr = h.heap
            debug_print(f"  heap[{i}]: {hex(heap_ptr.value if heap_ptr else 0)} (from DescriptorHeap)")
            raw.append(heap_ptr)
        elif isinstance(h, ctypes.c_void_p):
            debug_print(f"  heap[{i}]: {hex(h.value)} (c_void_p)")
            raw.append(h)
        elif h is None:
            debug_print(f"  heap[{i}]: None (skipping)")
            continue
        else:
            # Пытаемся преобразовать в c_void_p
            try:
                val = int(h)
                debug_print(f"  heap[{i}]: {hex(val)} (converted from int)")
                raw.append(ctypes.c_void_p(val))
            except:
                debug_print(f"  heap[{i}]: cannot convert {type(h)} to int")
                continue

    if not raw:
        debug_print("  No valid heaps to set")
        return

    count = len(raw)
    array_type = ctypes.c_void_p * count
    try:
        _set_descriptor_heaps(ctypes.c_size_t(count), array_type(*raw))
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()


def set_render_target(rtv: int) -> None:
    """Если в DLL функция ``set_render_target`` отсутствует – используем ``set_render_targets``."""
    debug_print(f"set_render_target(rtv={hex(rtv)})")
    if _set_render_target:
        try:
            _set_render_target(ctypes.c_uintptr(rtv))
            debug_print("  -> OK (using set_render_target)")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  -> using set_render_targets fallback")
        set_render_targets((rtv,))


def set_render_targets(rtvs: Tuple[int, ...]) -> None:
    debug_print(f"set_render_targets(count={len(rtvs)})")
    for i, rtv in enumerate(rtvs):
        debug_print(f"  rtv[{i}]: {hex(rtv)}")

    count = len(rtvs)
    array_type = ctypes.c_uintptr * count
    try:
        _set_render_targets(ctypes.c_size_t(count), array_type(*[ctypes.c_uintptr(r) for r in rtvs]))
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")
        traceback.print_exc()


def clear_render_target(rtv: int, color: Tuple[float, float, float, float]) -> None:
    debug_print(f"clear_render_target(rtv={hex(rtv)}, color={color})")
    rgba = (ctypes.c_float * 4)(*color)
    try:
        _clear_render_target(ctypes.c_uintptr(rtv), rgba)
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def set_viewport(
        x: int,
        y: int,
        w: int,
        h: int,
        min_depth: float = 0.0,
        max_depth: float = 1.0,
) -> None:
    debug_print(f"set_viewport(x={x}, y={y}, w={w}, h={h}, min_depth={min_depth}, max_depth={max_depth})")
    try:
        _set_viewport(
            ctypes.c_int(x),
            ctypes.c_int(y),
            ctypes.c_int(w),
            ctypes.c_int(h),
            ctypes.c_float(min_depth),
            ctypes.c_float(max_depth),
        )
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> None:
    debug_print(f"set_scissor_rect(left={left}, top={top}, right={right}, bottom={bottom})")
    try:
        _set_scissor_rect(
            ctypes.c_int(left), ctypes.c_int(top), ctypes.c_int(right), ctypes.c_int(bottom)
        )
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def set_vertex_buffers(vertex_buffer: ctypes.c_void_p, index_buffer: Optional[ctypes.c_void_p] = None) -> None:
    debug_print(f"set_vertex_buffers(vertex={hex(vertex_buffer.value if vertex_buffer else 0)}, "
                f"index={hex(index_buffer.value if index_buffer else 0)})")
    ib = index_buffer if index_buffer is not None else ctypes.c_void_p()
    try:
        _set_vertex_buffers(_to_cvoid(vertex_buffer), _to_cvoid(ib))
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def draw_instanced(
        vertex_count: int,
        instance_count: int = 1,
        start_vertex: int = 0,
        start_instance: int = 0,
) -> None:
    debug_print(f"draw_instanced(vertex_count={vertex_count}, instance_count={instance_count}, "
                f"start_vertex={start_vertex}, start_instance={start_instance})")
    try:
        _draw_instanced(
            ctypes.c_uint(vertex_count),
            ctypes.c_uint(instance_count),
            ctypes.c_uint(start_vertex),
            ctypes.c_uint(start_instance),
        )
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def draw_indexed_instanced(
        index_count: int,
        instance_count: int = 1,
        start_index: int = 0,
        base_vertex: int = 0,
        start_instance: int = 0,
) -> None:
    debug_print(f"draw_indexed_instanced(index_count={index_count}, instance_count={instance_count}, "
                f"start_index={start_index}, base_vertex={base_vertex}, start_instance={start_instance})")
    try:
        _draw_indexed_instanced(
            ctypes.c_uint(index_count),
            ctypes.c_uint(instance_count),
            ctypes.c_uint(start_index),
            ctypes.c_int(base_vertex),
            ctypes.c_uint(start_instance),
        )
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def wait_for_gpu() -> None:
    debug_print("wait_for_gpu()")
    try:
        _wait_for_gpu()
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def release_resource(resource: Any) -> None:
    debug_print(f"release_resource(resource={hex(resource.value if resource else 0)})")
    if not resource:
        debug_print("  SKIPPED: no resource")
        return
    try:
        _release_resource(_to_cvoid(resource))
        debug_print("  -> OK")
    except Exception as e:
        debug_print(f"  ERROR: {e}")


def get_frame_index() -> int:
    try:
        result = _get_frame_index()
        debug_print(f"get_frame_index() -> {result}")
        return result
    except Exception as e:
        debug_print(f"get_frame_index() ERROR: {e}")
        return 0


def get_rtv_descriptor_size() -> int:
    try:
        result = _get_rtv_descriptor_size()
        debug_print(f"get_rtv_descriptor_size() -> {result}")
        return result
    except Exception as e:
        debug_print(f"get_rtv_descriptor_size() ERROR: {e}")
        return 0


def get_dsv_descriptor_size() -> int:
    try:
        result = _get_dsv_descriptor_size()
        debug_print(f"get_dsv_descriptor_size() -> {result}")
        return result
    except Exception as e:
        debug_print(f"get_dsv_descriptor_size() ERROR: {e}")
        return 0


def set_vsync(enable: bool) -> None:
    """Пока stub – в DX12‑бекенде V‑sync реализуется через параметр sync_interval."""
    debug_print(f"set_vsync(enable={enable})")
    if _set_vsync:
        try:
            _set_vsync(ctypes.c_bool(enable))
            debug_print("  -> OK")
        except Exception as e:
            debug_print(f"  ERROR: {e}")
    else:
        debug_print("  -> function not available")


# ----------------------------------------------------------------------
# Что экспортируем
# ----------------------------------------------------------------------
__all__ = [
    "SWAP_CHAIN_BUFFER_COUNT",
    "DXGI_FORMAT_R8G8B8A8_UNORM",
    # low‑level функции
    "create_device",
    "create_command_queue",
    "create_swap_chain",
    "resize_swap_chain",
    "present_swap_chain",
    "compile_shader",
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
    "create_constant_buffer_view",  # <<‑ NEW
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
    "set_vsync",
    "DEBUG",
]