# alkash3d/__init__.py
"""
AlKAsH3D Game Engine – публичный API.
"""

from alkash3d.utils import logger
from alkash3d.engine import Engine
from alkash3d.window import Window
from alkash3d.scene import (
    Scene, Camera, DirectionalLight, PointLight, SpotLight, Mesh, Model, Node,
)
from alkash3d.math import Vec3, Vec4, Mat4, Quat
from alkash3d.assets.material import PBRMaterial
from alkash3d.assets.texture_manager import TextureManager
from alkash3d.renderer import (
    ForwardRenderer,
    DeferredRenderer,
    HybridRenderer,
    RTXRenderer,
)

# ❗ FIX: select_backend находится в модуле alkash3d.graphics.backend,
# а не в корневом пакете graphics.
from alkash3d.graphics.backend import select_backend

__version__ = "2.0.0"

__all__ = [
    "Engine",
    "Window",
    "Scene",
    "Camera",
    "DirectionalLight",
    "PointLight",
    "SpotLight",
    "Mesh",
    "Model",
    "Node",
    "Vec3",
    "Vec4",
    "Mat4",
    "Quat",
    "PBRMaterial",
    "TextureManager",
    "ForwardRenderer",
    "DeferredRenderer",
    "HybridRenderer",
    "RTXRenderer",
    "select_backend",
]
