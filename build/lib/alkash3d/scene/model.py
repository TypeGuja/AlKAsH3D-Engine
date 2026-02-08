# alkas3d/scene/model.py
# ---------------------------------------------------------------
# Простейший контейнер, собирающий один или несколько Mesh‑ов.
# ---------------------------------------------------------------
from alkash3d.scene.node import Node

class Model(Node):
    """
    Объединяет несколько Mesh‑ов в один объект‑модель.
    Удобно для загрузки сложных .obj‑файлов, состоящих из нескольких частей.
    """
    def __init__(self, meshes, name="Model"):
        super().__init__(name)
        self.meshes = meshes          # список Mesh‑ов
        for m in meshes:
            self.add_child(m)

    def draw(self):
        for m in self.meshes:
            m.draw()
