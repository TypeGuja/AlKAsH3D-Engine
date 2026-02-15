"""
Пакет scene – узлы сцены (Node), камера, свет, меши, модели.
"""

from alkash3d.scene.node import Node
from alkash3d.scene.camera import Camera
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight
from alkash3d.scene.mesh import Mesh
from alkash3d.scene.model import Model
from alkash3d.scene.scene import Scene

__all__ = ["Node", "Camera", "DirectionalLight", "PointLight",
           "SpotLight", "Mesh", "Model", "Scene"]
