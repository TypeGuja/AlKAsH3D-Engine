# alkash3d/scene/scene.py
"""
Корневой узел сцены с поддержкой Octree‑culling.
ИСПРАВЛЕННАЯ ВЕРСИЯ с включенным culling для отладки
"""

from alkash3d.scene.node import Node
from alkash3d.culling.octree import Octree
from alkash3d.utils.logger import logger


class Scene(Node):
    """Корневой узел сцены с поддержкой Octree‑culling."""
    def __init__(self):
        super().__init__("RootScene")
        self.culling = Octree(
            bounds=((-50.0, -50.0, -50.0), (50.0, 50.0, 50.0)),
            max_depth=6,
            max_objects=8,
        )
        logger.debug("[Scene] Created with octree")

    def update(self, dt):
        """Обновляет все узлы сцены."""
        updated = 0
        for node in self.traverse():
            if hasattr(node, "on_update"):
                try:
                    node.on_update(dt)
                    updated += 1
                except Exception as e:
                    logger.error(f"[Scene] Error updating node {node.name}: {e}")

        logger.debug(f"[Scene] Updated {updated} nodes")

        # Обновляем octree для корректного culling
        try:
            self.culling.rebuild(self)
            logger.debug("[Scene] Octree rebuilt successfully")
        except Exception as e:
            logger.error(f"[Scene] Culling rebuild error: {e}")

    def visible_nodes(self, camera):
        """Возвращает видимые узлы сцены."""
        try:
            # Сначала обновляем octree, если нужно
            # self.culling.rebuild(self)  # rebuild уже вызывается в update
            
            frustum = camera.get_view_projection_frustum()
            visible = self.culling.query(frustum)
            logger.debug(f"[Scene] Visible nodes: {len(visible)}")
            return visible
        except Exception as e:
            logger.error(f"[Scene] visible_nodes error: {e}")
            # В случае ошибки возвращаем все узлы
            all_nodes = list(self.traverse())
            logger.debug(f"[Scene] Fallback - returning all {len(all_nodes)} nodes")
            return all_nodes