"""
Graphics utilities package.
"""

from alkash3d.graphics.utils import d3d12_wrapper as dx
from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap

__all__ = [
    "dx",
    "DescriptorHeap",
]
