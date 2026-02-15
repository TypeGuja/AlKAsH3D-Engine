"""
Минимальная заглушка BVH‑структуры.
"""

class BVH:
    def __init__(self):
        self.root = None

    def build(self, meshes):
        """Построить BVH из списка Mesh‑ов (заглушка)."""
        self.root = meshes

    def intersect(self, ray_origin, ray_dir):
        """Вернуть первый пересекающий объект (заглушка)."""
        return None
