# alkash3d/__init__.py
"""
AlKAsH3D Game Engine
A modern 3D game engine with OpenGL rendering support.
"""

from alkash3d.engine import Engine
from alkash3d.window import Window
from alkash3d.scene import Scene
from alkash3d.scene.camera import Camera
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight
from alkash3d.scene.mesh import Mesh
from alkash3d.scene.model import Model
from alkash3d.math.vec3 import Vec3
from alkash3d.math.vec4 import Vec4
from alkash3d.math.mat4 import Mat4
from alkash3d.math.quat import Quat

# Версия пакета
__version__ = "1.0.0"

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
    "Vec3",
    "Vec4",
    "Mat4",
    "Quat"
]
