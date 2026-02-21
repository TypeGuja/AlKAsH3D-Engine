# alkash3d/graphics/utils/d3d12_wrapper.py
# -*- coding: utf-8 -*-
"""
Безопасная обёртка для нативной библиотеки (DX12 helper).
Загружает функции из DLL (Rust/C) и предоставляет удобные Python-обёртки.
Фокус — корректная обработка указателей и безопасный fallback при ошибках.
"""

from __future__ import annotations
import ctypes
import os
import sys
from pathlib import Path
from typing import Any, Optional, Sequence

from alkash3d.utils import logger

DEBUG = True


def debug_print(*args, **kwargs):
    if DEBUG:
        print("[D3D12_WRAPPER]", *args, **kwargs)


# ----------------------------------------------------------------------
# Константы
# ----------------------------------------------------------------------
SWAP_CHAIN_BUFFER_COUNT = 2
DXGI_FORMAT_R8G8B8A8_UNORM = 28

# Имя DLL — ожидается рядом с пакетом или в рабочей директории
_ext = ".dll" if sys.platform.startswith("win") else ".so"
_DEFAULT_DLL_NAME = f"alkash3d_dx12{_ext}"

_lib: Optional[ctypes.CDLL] = None
_lib_path: Optional[str] = None


def _locate_dll() -> Optional[str]:
    """Найти DLL в нескольких стандартных местах."""
    global _lib_path
    if _lib_path:
        return _lib_path

    base_dir = Path(__file__).parent
    candidates = [
        base_dir / _DEFAULT_DLL_NAME,
        base_dir.parent.parent / _DEFAULT_DLL_NAME,  # корень проекта
        Path.cwd() / _DEFAULT_DLL_NAME,
        Path(_DEFAULT_DLL_NAME),
    ]

    for c in candidates:
        if c.exists():
            _lib_path = str(c.absolute())
            logger.debug(f"[d3d12_wrapper] Found DLL at: {_lib_path}")
            return _lib_path

    logger.warning(f"[d3d12_wrapper] DLL not found in candidates: {candidates}")
    return None


def _load_lib() -> Optional[ctypes.CDLL]:
    """Загрузить библиотеку, если ещё не загружена."""
    global _lib
    if _lib is not None:
        return _lib

    libpath = _locate_dll()
    if libpath is None:
        logger.warning("[d3d12_wrapper] Native DLL not found; operating in stub mode")
        return None

    try:
        _lib = ctypes.CDLL(libpath)
        logger.info(f"[d3d12_wrapper] Loaded native library: {libpath}")
        return _lib
    except Exception as e:
        logger.error(f"[d3d12_wrapper] Failed to load {libpath}: {e}")
        _lib = None
        return None


def _load_func(name: str, restype, argtypes, required: bool = False):
    """Загрузить функцию из DLL с проверкой типов."""
    lib = _load_lib()
    if lib is None:
        if required:
            raise RuntimeError(f"[d3d12_wrapper] Required native library not loaded for function {name}")
        return None

    try:
        fn = getattr(lib, name)
        fn.restype = restype
        fn.argtypes = argtypes
        logger.debug(f"[d3d12_wrapper] Loaded function '{name}'")
        return fn
    except AttributeError:
        if required:
            raise RuntimeError(f"[d3d12_wrapper] Required function '{name}' not exported by native library")
        logger.debug(f"[d3d12_wrapper] Function '{name}' not found in native library")
        return None


# Для совместимости с более старыми версиями Python
if not hasattr(ctypes, "c_uintptr"):
    ctypes.c_uintptr = ctypes.c_void_p

# ----------------------------------------------------------------------
# Загрузка всех функций
# ----------------------------------------------------------------------
# Основные функции устройства
_create_device = _load_func("create_device", ctypes.c_void_p, [], required=False)
_create_command_queue = _load_func("create_command_queue", ctypes.c_void_p, [ctypes.c_void_p], required=False)
_release_resource = _load_func("release_resource", None, [ctypes.c_void_p], required=False)
_force_cleanup = _load_func("force_cleanup", None, [], required=False)

# Swap chain функции
_create_swap_chain = _load_func(
    "create_swap_chain",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint],
    required=False
)
_swap_chain_get_buffer = _load_func(
    "swap_chain_get_buffer",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint],
    required=False
)
_resize_swap_chain = _load_func(
    "resize_swap_chain",
    None,
    [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint],
    required=False
)
_present_swap_chain = _load_func(
    "present_swap_chain",
    None,
    [ctypes.c_void_p, ctypes.c_uint],
    required=False
)

# Функции кадра
_begin_frame = _load_func("begin_frame", None, [], required=False)
_end_frame = _load_func("end_frame", ctypes.c_bool, [], required=False)
_wait_for_gpu = _load_func("wait_for_gpu", None, [], required=False)
_get_frame_index = _load_func("get_frame_index", ctypes.c_uint, [], required=False)

# Шейдеры
_compile_shader = _load_func(
    "compile_shader",
    ctypes.c_int,
    [ctypes.c_wchar_p, ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)],
    required=False,
)
_create_graphics_ps = _load_func(
    "create_graphics_ps",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=False,
)
_set_graphics_pipeline = _load_func(
    "set_graphics_pipeline",
    None,
    [ctypes.c_void_p],
    required=False
)

# Буферы
_create_buffer = _load_func(
    "create_buffer",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p],
    required=False,
)
_update_subresource = _load_func(
    "update_subresource",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t],
    required=False
)

# Текстуры
_create_texture_from_memory = _load_func(
    "create_texture_from_memory",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_char_p],
    required=False,
)
_update_texture = _load_func(
    "update_texture",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint],
    required=False
)

# Дескрипторные хипы
_create_descriptor_heap = _load_func(
    "create_descriptor_heap",
    ctypes.c_void_p,
    [ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_bool],
    required=False,
)
_GetCPUDescriptorHandleForHeapStart = _load_func(
    "GetCPUDescriptorHandleForHeapStart",
    ctypes.c_uintptr,
    [ctypes.c_void_p],
    required=False,
)
_GetGPUDescriptorHandleForHeapStart = _load_func(
    "GetGPUDescriptorHandleForHeapStart",
    ctypes.c_uintptr,
    [ctypes.c_void_p],
    required=False,
)
_offset_descriptor_handle = _load_func(
    "offset_descriptor_handle",
    ctypes.c_uintptr,
    [ctypes.c_uintptr, ctypes.c_uint],
    required=False
)

# Views
_create_shader_resource_view = _load_func(
    "create_shader_resource_view",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=False,
)
_create_render_target_view = _load_func(
    "create_render_target_view",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=False,
)
_create_constant_buffer_view = _load_func(
    "create_constant_buffer_view",
    None,
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p],
    required=False,
)

# Команды рендеринга
_set_root_descriptor_table = _load_func(
    "set_root_descriptor_table",
    None,
    [ctypes.c_uint, ctypes.c_uintptr],
    required=False
)
_set_descriptor_heaps = _load_func(
    "set_descriptor_heaps",
    None,
    [ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)],
    required=False,
)
_set_render_target = _load_func(
    "set_render_target",
    None,
    [ctypes.c_uintptr],
    required=False
)
_set_render_targets = _load_func(
    "set_render_targets",
    None,
    [ctypes.c_uint, ctypes.POINTER(ctypes.c_uintptr)],
    required=False,
)
_clear_render_target = _load_func(
    "clear_render_target",
    None,
    [ctypes.c_uintptr, ctypes.POINTER(ctypes.c_float)],
    required=False
)
_set_viewport = _load_func(
    "set_viewport",
    None,
    [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_float, ctypes.c_float],
    required=False,
)
_set_scissor_rect = _load_func(
    "set_scissor_rect",
    None,
    [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int],
    required=False
)
_set_vertex_buffers = _load_func(
    "set_vertex_buffers",
    None,
    [ctypes.c_void_p, ctypes.c_void_p],
    required=False
)
_draw_instanced = _load_func(
    "draw_instanced",
    None,
    [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint],
    required=False,
)
_draw_indexed_instanced = _load_func(
    "draw_indexed_instanced",
    None,
    [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_uint],
    required=False,
)

# Информационные функции
_get_rtv_descriptor_size = _load_func("get_rtv_descriptor_size", ctypes.c_uint, [], required=False)
_get_dsv_descriptor_size = _load_func("get_dsv_descriptor_size", ctypes.c_uint, [], required=False)
_set_vsync = _load_func("set_vsync", None, [ctypes.c_bool], required=False)


# ----------------------------------------------------------------------
# Вспомогательные функции
# ----------------------------------------------------------------------
def _to_cvoid(ptr: Any) -> ctypes.c_void_p:
    """Преобразовать любой указатель/число в ctypes.c_void_p."""
    if ptr is None:
        return ctypes.c_void_p()
    if isinstance(ptr, ctypes.c_void_p):
        return ptr
    try:
        return ctypes.c_void_p(int(ptr))
    except (TypeError, ValueError):
        return ctypes.c_void_p()


# ----------------------------------------------------------------------
# Основные API функции
# ----------------------------------------------------------------------
def create_device() -> ctypes.c_void_p:
    """Создать DirectX 12 устройство."""
    if _create_device is None:
        logger.debug("[d3d12_wrapper] create_device not available")
        return ctypes.c_void_p(0)
    try:
        res = _create_device()
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_device failed: {e}")
        return ctypes.c_void_p(0)


def create_command_queue(device: ctypes.c_void_p) -> ctypes.c_void_p:
    """Создать командную очередь."""
    if _create_command_queue is None:
        logger.debug("[d3d12_wrapper] create_command_queue not available")
        return ctypes.c_void_p(0)
    try:
        res = _create_command_queue(_to_cvoid(device))
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_command_queue failed: {e}")
        return ctypes.c_void_p(0)


def create_swap_chain(command_queue: ctypes.c_void_p, hwnd: int, width: int, height: int) -> ctypes.c_void_p:
    """Создать swap chain."""
    if _create_swap_chain is None:
        logger.debug("[d3d12_wrapper] create_swap_chain not available")
        return ctypes.c_void_p(0)
    try:
        res = _create_swap_chain(
            _to_cvoid(command_queue),
            ctypes.c_void_p(hwnd),
            ctypes.c_uint(width),
            ctypes.c_uint(height)
        )
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_swap_chain failed: {e}")
        return ctypes.c_void_p(0)


def swap_chain_get_buffer(swap_chain: ctypes.c_void_p, index: int) -> ctypes.c_void_p:
    """Получить back buffer из swap chain."""
    if _swap_chain_get_buffer is None:
        logger.debug("[d3d12_wrapper] swap_chain_get_buffer not available")
        return ctypes.c_void_p(0)
    try:
        res = _swap_chain_get_buffer(_to_cvoid(swap_chain), ctypes.c_uint(index))
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] swap_chain_get_buffer failed: {e}")
        return ctypes.c_void_p(0)


def resize_swap_chain(swap_chain: ctypes.c_void_p, width: int, height: int) -> None:
    """Изменить размер swap chain."""
    if _resize_swap_chain is None:
        logger.debug("[d3d12_wrapper] resize_swap_chain not available")
        return
    try:
        _resize_swap_chain(_to_cvoid(swap_chain), ctypes.c_uint(width), ctypes.c_uint(height))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] resize_swap_chain failed: {e}")


def present_swap_chain(swap_chain: ctypes.c_void_p, sync_interval: int = 1) -> None:
    """Показать кадр."""
    if _present_swap_chain is None:
        logger.debug("[d3d12_wrapper] present_swap_chain not available")
        return
    try:
        _present_swap_chain(_to_cvoid(swap_chain), ctypes.c_uint(sync_interval))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] present_swap_chain failed: {e}")


def begin_frame() -> None:
    """Начать новый кадр."""
    if _begin_frame is None:
        logger.debug("[d3d12_wrapper] begin_frame not available")
        return
    try:
        _begin_frame()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] begin_frame failed: {e}")


def end_frame() -> bool:
    """Завершить кадр."""
    if _end_frame is None:
        logger.debug("[d3d12_wrapper] end_frame not available")
        return False
    try:
        return bool(_end_frame())
    except Exception as e:
        logger.error(f"[d3d12_wrapper] end_frame failed: {e}")
        return False


def wait_for_gpu() -> None:
    """Ожидать завершения работы GPU."""
    if _wait_for_gpu is None:
        logger.debug("[d3d12_wrapper] wait_for_gpu not available")
        return
    try:
        _wait_for_gpu()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] wait_for_gpu failed: {e}")


def get_frame_index() -> int:
    """Получить текущий индекс кадра."""
    if _get_frame_index is None:
        return 0
    try:
        return _get_frame_index()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_frame_index failed: {e}")
        return 0


def compile_shader(file_path: str, entry_point: str, profile: str) -> int:
    """Скомпилировать шейдер."""
    if _compile_shader is None:
        logger.debug("[d3d12_wrapper] compile_shader not available")
        return 0

    if not os.path.isfile(file_path):
        logger.error(f"[d3d12_wrapper] Shader file not found: {file_path}")
        return 0

    try:
        entry_c = ctypes.c_char_p(entry_point.encode("utf-8"))
        profile_c = ctypes.c_char_p(profile.encode("utf-8"))
        out_blob = ctypes.c_void_p()

        hr = _compile_shader(file_path, entry_c, profile_c, ctypes.byref(out_blob))
        if hr != 0:
            logger.error(f"[d3d12_wrapper] Shader compilation failed: HRESULT {hr:#x}")
            return 0

        return out_blob.value or 0
    except Exception as e:
        logger.error(f"[d3d12_wrapper] compile_shader failed: {e}")
        return 0


def create_graphics_ps(device: ctypes.c_void_p, vs_blob: int, ps_blob: int) -> ctypes.c_void_p:
    """Создать graphics pipeline state object."""
    if _create_graphics_ps is None:
        logger.debug("[d3d12_wrapper] create_graphics_ps not available")
        return ctypes.c_void_p(0)
    try:
        res = _create_graphics_ps(
            _to_cvoid(device),
            ctypes.c_void_p(vs_blob),
            ctypes.c_void_p(ps_blob)
        )
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_graphics_ps failed: {e}")
        return ctypes.c_void_p(0)


def set_graphics_pipeline(pso: ctypes.c_void_p) -> None:
    """Установить PSO."""
    if _set_graphics_pipeline is None:
        logger.debug("[d3d12_wrapper] set_graphics_pipeline not available")
        return
    try:
        _set_graphics_pipeline(_to_cvoid(pso))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_graphics_pipeline failed: {e}")


def create_buffer(device: ctypes.c_void_p, size: int, usage: str = "default") -> ctypes.c_void_p:
    """Создать буфер."""
    if _create_buffer is None:
        logger.debug("[d3d12_wrapper] create_buffer not available")
        return ctypes.c_void_p(0)

    if not device or size <= 0:
        return ctypes.c_void_p(0)

    try:
        # Обрабатываем как строку, так и bytes
        if isinstance(usage, bytes):
            usage_bytes = usage
        else:
            usage_bytes = usage.encode("utf-8")

        res = _create_buffer(_to_cvoid(device), ctypes.c_size_t(size), usage_bytes)
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_buffer failed: {e}")
        return ctypes.c_void_p(0)


def update_subresource(buffer: Any, data: bytes, size: int = None) -> None:
    """Обновить данные в буфере."""
    if _update_subresource is None:
        logger.debug("[d3d12_wrapper] update_subresource not available")
        return

    if not buffer or not data:
        return

    if size is None:
        size = len(data)

    try:
        raw = ctypes.create_string_buffer(data, size)
        _update_subresource(_to_cvoid(buffer), ctypes.addressof(raw), ctypes.c_size_t(size))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_subresource failed: {e}")


def create_texture_from_memory(
        device: ctypes.c_void_p,
        data: Optional[bytes],
        width: int,
        height: int,
        fmt: str | bytes = "rgba8",
) -> ctypes.c_void_p:
    """Создать текстуру из данных в памяти."""
    if _create_texture_from_memory is None:
        logger.debug("[d3d12_wrapper] create_texture_from_memory not available")
        return ctypes.c_void_p(0)

    if not device or width <= 0 or height <= 0:
        return ctypes.c_void_p(0)

    try:
        fmt_bytes = fmt if isinstance(fmt, bytes) else str(fmt).encode("utf-8")

        data_ptr = ctypes.c_void_p()
        if data:
            buf = ctypes.create_string_buffer(data, len(data))
            data_ptr = ctypes.c_void_p(ctypes.addressof(buf))

        res = _create_texture_from_memory(
            _to_cvoid(device),
            data_ptr,
            ctypes.c_uint(width),
            ctypes.c_uint(height),
            ctypes.c_char_p(fmt_bytes),
        )
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_texture_from_memory failed: {e}")
        return ctypes.c_void_p(0)


def update_texture(texture: ctypes.c_void_p, data: bytes, width: int, height: int) -> None:
    """Обновить данные в текстуре."""
    if _update_texture is None:
        logger.debug("[d3d12_wrapper] update_texture not available")
        return

    if not texture or not data:
        return

    try:
        buf = ctypes.create_string_buffer(data, len(data))
        _update_texture(_to_cvoid(texture), ctypes.addressof(buf), width, height)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] update_texture failed: {e}")


def create_descriptor_heap(
        device: ctypes.c_void_p,
        num_descriptors: int,
        heap_type: int,
        shader_visible: bool = False
) -> ctypes.c_void_p:
    """Создать дескрипторную кучу."""
    if _create_descriptor_heap is None:
        logger.debug("[d3d12_wrapper] create_descriptor_heap not available")
        return ctypes.c_void_p(0)

    if not device or num_descriptors <= 0:
        return ctypes.c_void_p(0)

    try:
        res = _create_descriptor_heap(
            _to_cvoid(device),
            ctypes.c_uint(num_descriptors),
            ctypes.c_uint(heap_type),
            ctypes.c_bool(shader_visible)
        )
        return _to_cvoid(res)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_descriptor_heap failed: {e}")
        return ctypes.c_void_p(0)


def GetCPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    """Получить CPU handle начала кучи."""
    if _GetCPUDescriptorHandleForHeapStart is None:
        return 0
    try:
        return _GetCPUDescriptorHandleForHeapStart(_to_cvoid(heap))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] GetCPUDescriptorHandleForHeapStart failed: {e}")
        return 0


def GetGPUDescriptorHandleForHeapStart(heap: ctypes.c_void_p) -> int:
    """Получить GPU handle начала кучи."""
    if _GetGPUDescriptorHandleForHeapStart is None:
        return 0
    try:
        return _GetGPUDescriptorHandleForHeapStart(_to_cvoid(heap))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] GetGPUDescriptorHandleForHeapStart failed: {e}")
        return 0


def offset_descriptor_handle(base: int, index: int) -> int:
    """Сместить дескриптор на index позиций."""
    if _offset_descriptor_handle is None:
        return base + (index * 32)  # fallback с размером по умолчанию
    try:
        return _offset_descriptor_handle(ctypes.c_uintptr(base), ctypes.c_uint(index))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] offset_descriptor_handle failed: {e}")
        return base + (index * 32)


def create_shader_resource_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int
) -> None:
    """Создать Shader Resource View."""
    if _create_shader_resource_view is None:
        logger.debug("[d3d12_wrapper] create_shader_resource_view not available")
        return
    try:
        _create_shader_resource_view(
            _to_cvoid(device),
            _to_cvoid(resource),
            ctypes.c_void_p(cpu_handle)
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_shader_resource_view failed: {e}")


def create_render_target_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: int
) -> None:
    """Создать Render Target View."""
    if _create_render_target_view is None:
        logger.debug("[d3d12_wrapper] create_render_target_view not available")
        return
    try:
        _create_render_target_view(
            _to_cvoid(device),
            _to_cvoid(resource),
            ctypes.c_void_p(cpu_handle)
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_render_target_view failed: {e}")


def create_constant_buffer_view(
        device: ctypes.c_void_p,
        resource: ctypes.c_void_p,
        cpu_handle: ctypes.c_void_p,
) -> None:
    """Создать Constant Buffer View."""
    if _create_constant_buffer_view is None:
        logger.debug("[d3d12_wrapper] create_constant_buffer_view not available")
        return
    try:
        _create_constant_buffer_view(_to_cvoid(device), _to_cvoid(resource), _to_cvoid(cpu_handle))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] create_constant_buffer_view failed: {e}")


def set_root_descriptor_table(root_index: int, gpu_handle: int) -> None:
    """Установить root descriptor table."""
    if _set_root_descriptor_table is None:
        logger.debug("[d3d12_wrapper] set_root_descriptor_table not available")
        return
    try:
        _set_root_descriptor_table(ctypes.c_uint(root_index), ctypes.c_uintptr(gpu_handle))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_root_descriptor_table failed: {e}")


def set_descriptor_heaps(heaps: Sequence[Any]) -> None:
    """
    Установить массивы дескрипторных куч.
    Формирует массив ctypes.c_void_p и вызывает нативную функцию.
    """
    if _set_descriptor_heaps is None:
        logger.debug("[d3d12_wrapper] set_descriptor_heaps not loaded — skipping")
        return

    ptrs = []
    for h in heaps:
        if h is None:
            continue

        # Получаем сырой указатель из разных типов объектов
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
            logger.debug(f"[d3d12_wrapper] Unknown heap object type: {type(h)} - skipping")
            continue

        ptrs.append(ctypes.c_void_p(val))

    if not ptrs:
        logger.debug("[d3d12_wrapper] No heaps to set — skipping native call")
        return

    try:
        arr = (ctypes.c_void_p * len(ptrs))(*ptrs)
        _set_descriptor_heaps(ctypes.c_uint(len(ptrs)), arr)
        logger.debug(f"[d3d12_wrapper] set_descriptor_heaps called with {len(ptrs)} heaps")
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_descriptor_heaps failed: {e}")


def set_render_target(rtv: int) -> None:
    """Установить один render target."""
    if _set_render_target is not None:
        try:
            _set_render_target(ctypes.c_uintptr(rtv))
        except Exception as e:
            logger.error(f"[d3d12_wrapper] set_render_target failed: {e}")
    elif _set_render_targets is not None:
        set_render_targets([rtv])
    else:
        logger.debug("[d3d12_wrapper] set_render_target(s) not available")


def set_render_targets(rtvs: Sequence[int]) -> None:
    """Установить несколько render targets."""
    if _set_render_targets is None:
        logger.debug("[d3d12_wrapper] set_render_targets not available")
        return

    if not rtvs:
        return

    try:
        count = len(rtvs)
        array_type = ctypes.c_uintptr * count
        _set_render_targets(
            ctypes.c_uint(count),
            array_type(*[ctypes.c_uintptr(r) for r in rtvs])
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_render_targets failed: {e}")


def clear_render_target(rtv: int, color: tuple[float, float, float, float]) -> None:
    """Очистить render target."""
    if _clear_render_target is None:
        logger.debug("[d3d12_wrapper] clear_render_target not available")
        return
    try:
        rgba = (ctypes.c_float * 4)(*color)
        _clear_render_target(ctypes.c_uintptr(rtv), rgba)
    except Exception as e:
        logger.error(f"[d3d12_wrapper] clear_render_target failed: {e}")


def set_viewport(
        x: int, y: int, w: int, h: int,
        min_depth: float = 0.0, max_depth: float = 1.0
) -> None:
    """Установить viewport."""
    if _set_viewport is None:
        logger.debug("[d3d12_wrapper] set_viewport not available")
        return
    try:
        _set_viewport(
            ctypes.c_int(x), ctypes.c_int(y), ctypes.c_int(w), ctypes.c_int(h),
            ctypes.c_float(min_depth), ctypes.c_float(max_depth)
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_viewport failed: {e}")


def set_scissor_rect(left: int, top: int, right: int, bottom: int) -> None:
    """Установить scissor rect."""
    if _set_scissor_rect is None:
        logger.debug("[d3d12_wrapper] set_scissor_rect not available")
        return
    try:
        _set_scissor_rect(
            ctypes.c_int(left), ctypes.c_int(top),
            ctypes.c_int(right), ctypes.c_int(bottom)
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_scissor_rect failed: {e}")


def set_vertex_buffers(
        vertex_buffer: ctypes.c_void_p,
        index_buffer: Optional[ctypes.c_void_p] = None
) -> None:
    """Установить vertex/index буферы."""
    if _set_vertex_buffers is None:
        logger.debug("[d3d12_wrapper] set_vertex_buffers not available")
        return
    try:
        ib = index_buffer if index_buffer is not None else ctypes.c_void_p()
        _set_vertex_buffers(_to_cvoid(vertex_buffer), _to_cvoid(ib))
    except Exception as e:
        logger.error(f"[d3d12_wrapper] set_vertex_buffers failed: {e}")


def draw_instanced(
        vertex_count: int,
        instance_count: int = 1,
        start_vertex: int = 0,
        start_instance: int = 0,
) -> None:
    """Нарисовать инстансы."""
    if _draw_instanced is None:
        logger.debug("[d3d12_wrapper] draw_instanced not available")
        return
    try:
        _draw_instanced(
            ctypes.c_uint(vertex_count),
            ctypes.c_uint(instance_count),
            ctypes.c_uint(start_vertex),
            ctypes.c_uint(start_instance),
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] draw_instanced failed: {e}")


def draw_indexed_instanced(
        index_count: int,
        instance_count: int = 1,
        start_index: int = 0,
        base_vertex: int = 0,
        start_instance: int = 0,
) -> None:
    """Нарисовать инстансы с индексами."""
    if _draw_indexed_instanced is None:
        logger.debug("[d3d12_wrapper] draw_indexed_instanced not available")
        return
    try:
        _draw_indexed_instanced(
            ctypes.c_uint(index_count),
            ctypes.c_uint(instance_count),
            ctypes.c_uint(start_index),
            ctypes.c_int(base_vertex),
            ctypes.c_uint(start_instance),
        )
    except Exception as e:
        logger.error(f"[d3d12_wrapper] draw_indexed_instanced failed: {e}")


def release_resource(resource: Any) -> None:
    """Освободить ресурс."""
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
    """Принудительная очистка всех ресурсов."""
    if _force_cleanup:
        try:
            _force_cleanup()
        except Exception as e:
            logger.error(f"[d3d12_wrapper] force_cleanup failed: {e}")


def get_rtv_descriptor_size() -> int:
    """Получить размер RTV дескриптора."""
    if _get_rtv_descriptor_size is None:
        return 32  # fallback
    try:
        return _get_rtv_descriptor_size()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_rtv_descriptor_size failed: {e}")
        return 32


def get_dsv_descriptor_size() -> int:
    """Получить размер DSV дескриптора."""
    if _get_dsv_descriptor_size is None:
        return 8  # fallback
    try:
        return _get_dsv_descriptor_size()
    except Exception as e:
        logger.error(f"[d3d12_wrapper] get_dsv_descriptor_size failed: {e}")
        return 8


def set_vsync(enable: bool) -> None:
    """Включить/выключить VSync."""
    if _set_vsync:
        try:
            _set_vsync(ctypes.c_bool(enable))
        except Exception as e:
            logger.error(f"[d3d12_wrapper] set_vsync failed: {e}")


# ----------------------------------------------------------------------
# Экспорт
# ----------------------------------------------------------------------
__all__ = [
    "SWAP_CHAIN_BUFFER_COUNT",
    "DXGI_FORMAT_R8G8B8A8_UNORM",
    # Основные функции
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
    # Шейдеры
    "compile_shader",
    "create_graphics_ps",
    "set_graphics_pipeline",
    # Буферы
    "create_buffer",
    "update_subresource",
    "release_resource",
    # Текстуры
    "create_texture_from_memory",
    "update_texture",
    # Дескрипторные хипы
    "create_descriptor_heap",
    "GetCPUDescriptorHandleForHeapStart",
    "GetGPUDescriptorHandleForHeapStart",
    "offset_descriptor_handle",
    "set_descriptor_heaps",
    # Views
    "create_shader_resource_view",
    "create_render_target_view",
    "create_constant_buffer_view",
    # Команды рендеринга
    "set_root_descriptor_table",
    "set_render_target",
    "set_render_targets",
    "clear_render_target",
    "set_viewport",
    "set_scissor_rect",
    "set_vertex_buffers",
    "draw_instanced",
    "draw_indexed_instanced",
    # Информационные функции
    "get_rtv_descriptor_size",
    "get_dsv_descriptor_size",
    "set_vsync",
    "DEBUG",
]