# -*- coding: utf-8 -*-
"""
Thin‑wrapper over the Rust crate ``alkash3d_dx12``.
"""

from __future__ import annotations

import ctypes
import os
import sys
from pathlib import Path
from typing import Callable, Any, Optional, Tuple

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print(*args, **kwargs)


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
    # ``None`` → NULL‑pointer
    if ptr is None:
        return ctypes.c_void_p()
    # ``int`` → привести к указателю
    return ctypes.c_void_p(ptr)


# ----------------------------------------------------------------------
# Универсальная загрузка функции из DLL.
# Если ``required`` == True и символ не найден → RuntimeError.
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
        return fn
    except AttributeError as e:
        if required:
            raise RuntimeError(
                f"[d3d12_wrapper] Required function '{name}' not exported from '{_lib_path}'"
            ) from e
        if logger:
            logger.debug(f"[d3d12_wrapper] Function '{name}' not found, skipping")
        return None


# ----------------------------------------------------------------------
# Загрузка всех функций из DLL (только те, которые действительно нужны)
# ----------------------------------------------------------------------
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
        ctypes.c_wchar_p,
        ctypes.c_char_p,
        ctypes.c_char_p,
        ctypes.POINTER(ctypes.c_void_p),
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
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_char_p,
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
    [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint],
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

# set_render_target – НЕ обязательна (есть fallback через set_render_targets)
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
#   High‑level API (более «дружественные» функции)
# ----------------------------------------------------------------------
def create_device() -> ctypes.c_void_p:
    result = _create_device()
    return _to_cvoid(result)


def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    if not device:
        return ctypes.c_void_p()
    result = _create_command_queue(device)
    return _to_cvoid(result)


def create_swap_chain(
    command_queue: ctypes.c_void_p, hwnd: int, width: int, height: int
) -> ctypes.c_void_p:
    if not command_queue or not hwnd:
        return ctypes.c_void_p()
    result = _create_swap_chain(
        command_queue,
        ctypes.c_uintptr(hwnd),
        ctypes.c_uint(width),
        ctypes.c_uint(height),
    )
    return _to_cvoid(result)


def resize_swap_chain(swap_chain: ctypes.c_void_p, width: int, height: int) -> None:
    if swap_chain:
        _resize_swap_chain(swap_chain, ctypes.c_uint(width), ctypes.c_uint(height))


def present_swap_chain(swap_chain: ctypes.c_void_p, sync_interval: int = 1) -> None:
    if swap_chain:
        _present_swap_chain(swap_chain, ctypes.c_uint(sync_interval))


def compile_shader(file_path: str, entry_point: str, profile: str) -> int:
    if not os.path.isfile(file_path):
        raise FileNotFoundError(file_path)

    src_utf16 = os.path.abspath(file_path)
    entry_c = ctypes.c_char_p(entry_point.encode("utf-8"))
    profile_c = ctypes.c_char_p(profile.encode("utf-8"))
    out_blob = ctypes.c_void_p()

    hr = _compile_shader(src_utf16, entry_c, profile_c, ctypes.byref(out_blob))
    if hr != 0:
        raise RuntimeError(f"Shader compilation failed with HRESULT {hr:#x}")
    return out_blob.value


def create_graphics_ps(
    device: ctypes.c_void_p, vs_blob: ctypes.c_void_p, ps_blob: ctypes.c_void_p
) -> ctypes.c_void_p:
    if not device or not vs_blob or not ps_blob:
        return ctypes.c_void_p()
    result = _create_graphics_ps(device, vs_blob, ps_blob)
    return _to_cvoid(result)


def set_graphics_pipeline(pso: ctypes.c_void_p) -> None:
    """Установить PSO (один вызов)."""
    if pso:
        _set_graphics_pipeline(pso)   # единственный вызов


def swap_chain_get_buffer(swap_chain: ctypes.c_void_p, buffer_index: int) -> ctypes.c_void_p:
    if not swap_chain:
        return ctypes.c_void_p()
    result = _swap_chain_get_buffer(swap_chain, ctypes.c_uint(buffer_index))
    return _to_cvoid(result)


def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    if not device or size <= 0:
        return ctypes.c_void_p()
    usage_bytes = usage.encode("utf-8")
    result = _create_buffer(device, ctypes.c_size_t(size), ctypes.c_char_p(usage_bytes))
    return _to_cvoid(result)


def update_subresource(buffer: Any, data: bytes) -> None:
    if not buffer or not data:
        return
    raw = ctypes.create_string_buffer(data, len(data))
    data_ptr = ctypes.c_void_p(ctypes.addressof(raw))
    _update_subresource(_to_cvoid(buffer), data_ptr, ctypes.c_size_t(len(data)))


def create_texture_from_memory(
    device: ctypes.c_void_p,
    data: Optional[bytes],
    width: int,
    height: int,
    fmt: str | bytes = "rgba8",
) -> ctypes.c_void_p:
    """
    Создать 2‑D‑текстуру. Параметр ``fmt`` может быть строкой
    (например, ``"RGBA8"``) или уже готовым ``bytes``‑значением
    (например, ``b"RGBA8"``).  Функция сама гарантирует, что передаёт
    корректный UTF‑8‑массив в нативную DLL.
    """
    if not device or width <= 0 or height <= 0:
        return ctypes.c_void_p()

    # -----------------------------------------------------------------
    # Приводим fmt к ``bytes`` – если уже ``bytes`` – оставляем как есть,
    # иначе кодируем в UTF‑8.
    # -----------------------------------------------------------------
    if isinstance(fmt, bytes):
        fmt_bytes = fmt               # уже готовый набор байт
    else:
        fmt_bytes = str(fmt).encode("utf-8")

    # -----------------------------------------------------------------
    # Подготовка указателя на данные (может быть None)
    # -----------------------------------------------------------------
    data_ptr = ctypes.c_void_p()
    if data:
        buf = ctypes.create_string_buffer(data, len(data))
        data_ptr = ctypes.c_void_p(ctypes.addressof(buf))

    # -----------------------------------------------------------------
    # Вызываем нативную функцию
    # -----------------------------------------------------------------
    result = _create_texture_from_memory(
        device,
        data_ptr,
        ctypes.c_uint(width),
        ctypes.c_uint(height),
        ctypes.c_char_p(fmt_bytes),
    )
    return _to_cvoid(result)


def update_texture(texture: ctypes.c_void_p, data: bytes, width: int, height: int) -> None:
    if not texture or not data:
        return
    buf = ctypes.create_string_buffer(data, len(data))
    data_ptr = ctypes.c_void_p(ctypes.addressof(buf))
    _update_texture(_to_cvoid(texture), data_ptr, ctypes.c_uint(width), ctypes.c_uint(height))


def create_descriptor_heap(
    device: ctypes.c_void_p, num_descriptors: int, heap_type: int
) -> ctypes.c_void_p:
    if not device or num_descriptors <= 0:
        raise RuntimeError("Invalid parameters for descriptor heap")
    result = _create_descriptor_heap(device, ctypes.c_uint(num_descriptors), ctypes.c_uint(heap_type))
    # ``result`` может быть как ``int``, так и ``c_void_p`` – приводим к ``c_void_p``.
    return _to_cvoid(result)


def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if not heap:
        return 0
    return _GetCPUDescriptorHandleForHeapStart(_to_cvoid(heap))


def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    if not heap:
        return 0
    return _GetGPUDescriptorHandleForHeapStart(_to_cvoid(heap))


def offset_descriptor_handle(base: int, index: int) -> int:
    return _offset_descriptor_handle(ctypes.c_uintptr(base), ctypes.c_uint(index))


def create_shader_resource_view(device: ctypes.c_void_p, resource: ctypes.c_void_p, cpu_handle: int) -> None:
    if device and resource and cpu_handle:
        _create_shader_resource_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle))


def create_render_target_view(device: ctypes.c_void_p, resource: ctypes.c_void_p, cpu_handle: int) -> None:
    if device and resource and cpu_handle:
        _create_render_target_view(_to_cvoid(device), _to_cvoid(resource), ctypes.c_void_p(cpu_handle))


def set_root_descriptor_table(root_index: int, gpu_handle: int) -> None:
    _set_root_descriptor_table(ctypes.c_uint(root_index), ctypes.c_uintptr(gpu_handle))


def set_descriptor_heaps(heaps: Tuple[ctypes.c_void_p, ...]) -> None:
    count = len(heaps)
    array_type = ctypes.c_void_p * count
    _set_descriptor_heaps(ctypes.c_size_t(count), array_type(*heaps))


def set_render_target(rtv: int) -> None:
    """
    Если в DLL функция ``set_render_target`` отсутствует – используем
    ``set_render_targets`` с единственным элементом.
    """
    if _set_render_target:
        _set_render_target(ctypes.c_uintptr(rtv))
    else:
        set_render_targets((rtv,))


def set_render_targets(rtvs: Tuple[int, ...]) -> None:
    count = len(rtvs)
    array_type = ctypes.c_uintptr * count
    _set_render_targets(ctypes.c_size_t(count), array_type(*[ctypes.c_uintptr(r) for r in rtvs]))


def clear_render_target(rtv: int, color: Tuple[float, float, float, float]) -> None:
    rgba = (ctypes.c_float * 4)(*color)
    _clear_render_target(ctypes.c_uintptr(rtv), rgba)


def set_viewport(
    x: int,
    y: int,
    w: int,
    h: int,
    min_depth: float = 0.0,
    max_depth: float = 1.0,
) -> None:
    _set_viewport(
        ctypes.c_int(x),
        ctypes.c_int(y),
        ctypes.c_int(w),
        ctypes.c_int(h),
        ctypes.c_float(min_depth),
        ctypes.c_float(max_depth),
    )


def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> None:
    _set_scissor_rect(
        ctypes.c_int(left),
        ctypes.c_int(top),
        ctypes.c_int(right),
        ctypes.c_int(bottom),
    )


def set_vertex_buffers(vertex_buffer: ctypes.c_void_p, index_buffer: Optional[ctypes.c_void_p] = None) -> None:
    ib = index_buffer if index_buffer is not None else ctypes.c_void_p()
    _set_vertex_buffers(_to_cvoid(vertex_buffer), _to_cvoid(ib))


def draw_instanced(
    vertex_count: int,
    instance_count: int = 1,
    start_vertex: int = 0,
    start_instance: int = 0,
) -> None:
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
    _draw_indexed_instanced(
        ctypes.c_uint(index_count),
        ctypes.c_uint(instance_count),
        ctypes.c_uint(start_index),
        ctypes.c_int(base_vertex),
        ctypes.c_uint(start_instance),
    )


def wait_for_gpu() -> None:
    _wait_for_gpu()


def release_resource(resource: Any) -> None:
    if not resource:
        return
    _release_resource(_to_cvoid(resource))


def get_frame_index() -> int:
    return _get_frame_index()


def get_rtv_descriptor_size() -> int:
    return _get_rtv_descriptor_size()


def get_dsv_descriptor_size() -> int:
    return _get_dsv_descriptor_size()


def set_vsync(enable: bool) -> None:
    """Пока stub – в DX12‑бекенде V‑sync реализуется через параметр sync_interval."""
    if _set_vsync:
        _set_vsync(ctypes.c_bool(enable))


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
