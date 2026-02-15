"""
Простая Octree‑структура для ускорения frustum‑culling.
"""

from __future__ import annotations

import numpy as np
from typing import Tuple, List

class OctreeNode:
    """Узел Octree – хранит объекты и (при необходимости) 8 дочерних узлов."""
    def __init__(self,
                 bounds: Tuple[Tuple[float, float, float], Tuple[float, float, float]],
                 depth: int = 0,
                 max_depth: int = 6,
                 max_objects: int = 8):
        self.bounds = (np.array(bounds[0], dtype=np.float32),
                       np.array(bounds[1], dtype=np.float32))
        self.depth = depth
        self.max_depth = max_depth
        self.max_objects = max_objects
        self.objects: List[object] = []
        self.children: List[OctreeNode] = []

    def insert(self, obj) -> None:
        """Вставить объект (должен иметь свойство `bounding_sphere`)."""
        if self.children:
            idx = self._get_child_index(obj)
            if idx is not None:
                self.children[idx].insert(obj)
                return

        self.objects.append(obj)

        if len(self.objects) > self.max_objects and self.depth < self.max_depth:
            self.subdivide()
            for o in self.objects[:]:
                idx = self._get_child_index(o)
                if idx is not None:
                    self.children[idx].insert(o)
                    self.objects.remove(o)

    def _get_child_index(self, obj) -> int | None:
        centre, radius = obj.bounding_sphere
        minb, maxb = self.bounds
        mid = (minb + maxb) * 0.5

        index = 0
        if centre[0] > mid[0]:
            index |= 1
        if centre[1] > mid[1]:
            index |= 2
        if centre[2] > mid[2]:
            index |= 4

        cmin, cmax = self._child_bounds(index)
        if np.all(centre - radius >= cmin) and np.all(centre + radius <= cmax):
            return index
        return None

    def _child_bounds(self, index: int) -> Tuple[np.ndarray, np.ndarray]:
        minb, maxb = self.bounds
        mid = (minb + maxb) * 0.5

        # x
        if index & 1:
            x0, x1 = mid[0], maxb[0]
        else:
            x0, x1 = minb[0], mid[0]
        # y
        if index & 2:
            y0, y1 = mid[1], maxb[1]
        else:
            y0, y1 = minb[1], mid[1]
        # z
        if index & 4:
            z0, z1 = mid[2], maxb[2]
        else:
            z0, z1 = minb[2], mid[2]

        return (np.array([x0, y0, z0], dtype=np.float32),
                np.array([x1, y1, z1], dtype=np.float32))

    def subdivide(self) -> None:
        for i in range(8):
            child_min, child_max = self._child_bounds(i)
            child = OctreeNode((child_min, child_max),
                               depth=self.depth + 1,
                               max_depth=self.max_depth,
                               max_objects=self.max_objects)
            self.children.append(child)

    def _intersects_frustum(self, frustum) -> bool:
        if frustum is None:
            return True

        minb, maxb = self.bounds
        for plane in frustum.planes:
            p = np.where(plane.normal >= 0, maxb, minb)
            if np.dot(plane.normal, p) + plane.distance < 0:
                return False
        return True

    def query(self, frustum) -> List[object]:
        """Возвратить все объекты, попадающие в frustum."""
        result = []
        if not self._intersects_frustum(frustum):
            return result

        if frustum is None:
            result.extend(self.objects)
        else:
            for obj in self.objects:
                centre, radius = obj.bounding_sphere
                if frustum.intersects_sphere(centre, radius):
                    result.append(obj)

        for child in self.children:
            result.extend(child.query(frustum))

        return result

class Octree:
    """Публичный API – создаём один объект Octree и работаем с ним."""
    def __init__(self,
                 bounds: Tuple[Tuple[float, float, float], Tuple[float, float, float]],
                 max_depth: int = 6,
                 max_objects: int = 8):
        self.root = OctreeNode(bounds, depth=0,
                               max_depth=max_depth,
                               max_objects=max_objects)

    def insert(self, obj) -> None:
        self.root.insert(obj)

    def clear(self) -> None:
        bounds = self.root.bounds
        self.root = OctreeNode(bounds, depth=0,
                               max_depth=self.root.max_depth,
                               max_objects=self.root.max_objects)

    def rebuild(self, scene_root) -> None:
        self.clear()
        for node in scene_root.traverse():
            if hasattr(node, "bounding_sphere"):
                self.insert(node)

    def query(self, frustum) -> List[object]:
        return self.root.query(frustum)