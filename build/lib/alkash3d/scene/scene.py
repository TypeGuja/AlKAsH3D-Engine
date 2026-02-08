# -*- coding: utf-8 -*-
"""Контейнерная сцена (корневой узел)"""
from alkash3d.scene.node import Node

class Scene(Node):
    """Корень сцены – обычный Node, но имеет метод update()."""
    def __init__(self):
        super().__init__("RootScene")

    def update(self, dt):
        # Проходим по всем узлам и вызываем их пользовательскую логику,
        # если она определена (например, анимация).
        for node in self.traverse():
            if hasattr(node, "on_update"):
                node.on_update(dt)
