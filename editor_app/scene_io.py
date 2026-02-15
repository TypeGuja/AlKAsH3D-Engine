# editor_app/scene_io.py
"""
Простая (де)сериализация сцены в/из JSON.
Поддерживает Mesh‑геометрию, материалы и основные Light‑параметры.
"""

import json
from pathlib import Path
from typing import Dict, Any

import numpy as np

from alkash3d.scene.node import Node
from alkash3d.scene.camera import Camera
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight
from alkash3d.scene.mesh import Mesh
from alkash3d.math.vec3 import Vec3


# ----------------------------------------------------------------------
def _vec3_to_list(v: Vec3) -> list[float]:
    return [float(v.x), float(v.y), float(v.z)]


def _list_to_vec3(lst: list[float]) -> Vec3:
    return Vec3(*lst)


# ----------------------------------------------------------------------
def node_to_dict(node: Node) -> Dict[str, Any]:
    """Рекурсивно переводит Node → словарь (для JSON)."""
    data = {
        "type": type(node).__name__,
        "name": node.name,
        "position": _vec3_to_list(node.position),
        "rotation": _vec3_to_list(node.rotation),
        "scale": _vec3_to_list(node.scale),
        "children": [node_to_dict(c) for c in node.children],
    }

    # --- Mesh специфично ------------------------------------------------
    if isinstance(node, Mesh):
        mesh_data = {
            "vertices": node.vertices.tolist() if hasattr(node, "vertices") else [],
            "indices": node.indices.tolist() if hasattr(node, "indices") else [],
            "normals": node.normals.tolist() if hasattr(node, "normals") else [],
            "tex_coords": node.tex_coords.tolist() if hasattr(node, "tex_coords") else [],
        }
        data["mesh"] = mesh_data

        # Простейший материал (цвет)
        if hasattr(node, "material") and hasattr(node.material, "color"):
            data["material"] = {"color": _vec3_to_list(node.material.color)}

    # --- Camera --------------------------------------------------------
    if isinstance(node, Camera):
        data["camera"] = {
            "fov": node.fov,
            "near": node.near,
            "far": node.far,
        }

    # --- Light ----------------------------------------------------------
    if isinstance(node, (DirectionalLight, PointLight, SpotLight)):
        light_data = {
            "intensity": getattr(node, "intensity", 1.0),
            "color": _vec3_to_list(getattr(node, "color", Vec3(1, 1, 1))),
        }
        if isinstance(node, SpotLight):
            light_data["spot_angle"] = getattr(node, "spot_angle", 30.0)
        data["light"] = light_data

    return data


def dict_to_node(data: Dict[str, Any]) -> Node:
    """Воссоздаёт Node (и вложенные) из словаря."""
    typ = data["type"]
    name = data.get("name", "Node")

    # Сопоставление типов
    type_map = {
        "Camera": Camera,
        "DirectionalLight": DirectionalLight,
        "PointLight": PointLight,
        "SpotLight": SpotLight,
        "Mesh": Mesh,
    }

    node: Node = type_map.get(typ, Node)()
    node.name = name

    # Трансформа
    node.position = _list_to_vec3(data["position"])
    node.rotation = _list_to_vec3(data["rotation"])
    node.scale = _list_to_vec3(data["scale"])

    # --- Mesh -----------------------------------------------------------
    if isinstance(node, Mesh) and "mesh" in data:
        m = data["mesh"]
        node.vertices = np.array(m.get("vertices", []), dtype=np.float32).reshape(-1, 3)
        node.indices = np.array(m.get("indices", []), dtype=np.uint32)
        if "normals" in m and m["normals"]:
            node.normals = np.array(m["normals"], dtype=np.float32).reshape(-1, 3)
        if "tex_coords" in m and m["tex_coords"]:
            node.tex_coords = np.array(m["tex_coords"], dtype=np.float32).reshape(-1, 2)

        # Material
        if "material" in data and hasattr(node, "material"):
            mat = data["material"]
            node.material.color = _list_to_vec3(mat["color"])

    # --- Camera --------------------------------------------------------
    if isinstance(node, Camera) and "camera" in data:
        cam = data["camera"]
        node.fov = cam.get("fov", node.fov)
        node.near = cam.get("near", node.near)
        node.far = cam.get("far", node.far)

    # --- Light ---------------------------------------------------------
    if isinstance(node, (DirectionalLight, PointLight, SpotLight)) and "light" in data:
        light = data["light"]
        node.intensity = light.get("intensity", getattr(node, "intensity", 1.0))
        node.color = _list_to_vec3(light.get("color", [1, 1, 1]))
        if isinstance(node, SpotLight):
            node.spot_angle = light.get("spot_angle", getattr(node, "spot_angle", 30.0))

    # Дети
    for child_data in data.get("children", []):
        child_node = dict_to_node(child_data)
        node.add_child(child_node)

    return node


# ----------------------------------------------------------------------
def save_scene(root: Node, path: Path) -> None:
    """Записывает сцену в JSON‑файл."""
    scene_data = {
        "version": "1.txt.0",
        "scene_name": root.name,
        "root": node_to_dict(root),
    }
    with Path(path).open("w", encoding="utf-8") as f:
        json.dump(scene_data, f, indent=2, ensure_ascii=False)


def load_scene(path: Path) -> Node:
    """Читает сцену из JSON‑файла."""
    with Path(path).open("r", encoding="utf-8") as f:
        raw = json.load(f)

    if "root" in raw:
        return dict_to_node(raw["root"])
    return dict_to_node(raw)