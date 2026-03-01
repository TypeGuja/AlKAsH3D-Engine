# alkash3d/graphics/__init__.py
# -*- coding: utf-8 -*-
"""
Graphics package – DX12 wrapper and descriptor‑heap helper.
"""

from .utils import d3d12_wrapper as dx
from .utils.descriptor_heap import DescriptorHeap

# ❗ FIX: экспортируем функцию select_backend, чтобы
#     `from alkash3d.graphics import select_backend` работало.
from .backend import select_backend

__all__ = [
    "dx",
    "DescriptorHeap",
    "select_backend",
]
