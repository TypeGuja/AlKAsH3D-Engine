# examples/cube_example.py
import alkash3d
import time


class CubeApp:
    def __init__(self):
        # Создаем окно и движок
        self.window = alkash3d.Window(800, 600, "Куб в AlKAsH3D")
        self.engine = alkash3d.Engine()

        # Инициализируем движок с окном
        self.engine.initialize(self.window)

        # Создаем сцену
        self.scene = alkash3d.Scene()

        # Создаем камеру
        self.camera = alkash3d.Camera()
        self.camera.set_position(alkash3d.Vec3(3, 2, 5))
        self.camera.look_at(alkash3d.Vec3(0, 0, 0))

        # Создаем освещение
        self.light = alkash3d.DirectionalLight()
        self.light.set_direction(alkash3d.Vec3(-1, -2, -1))
        self.light.set_color(alkash3d.Vec3(1, 1, 1))
        self.light.set_intensity(1.0)
        self.scene.add_light(self.light)

        # Создаем куб
        self.cube = self.create_cube()
        self.scene.add_node(self.cube)

        # Переменные для анимации
        self.rotation = 0

    def create_cube(self):
        # Создаем узел для куба
        cube_node = alkash3d.Node("Cube")

        # Создаем меш для куба
        mesh = alkash3d.Mesh()

        # Вершины куба (позиция + нормаль + текстурные координаты)
        vertices = [
            # Передняя грань (z = 1)
            -1.0, -1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
            1.0, -1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0,
            1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0,
            -1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0,

            # Задняя грань (z = -1)
            -1.0, -1.0, -1.0, 0.0, 0.0, -1.0, 1.0, 0.0,
            1.0, -1.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0,
            1.0, 1.0, -1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
            -1.0, 1.0, -1.0, 0.0, 0.0, -1.0, 1.0, 1.0,

            # Левая грань (x = -1)
            -1.0, -1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0,
            -1.0, -1.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0,
            -1.0, 1.0, 1.0, -1.0, 0.0, 0.0, 1.0, 1.0,
            -1.0, 1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 1.0,

            # Правая грань (x = 1)
            1.0, -1.0, -1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
            1.0, -1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
            1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0,
            1.0, 1.0, -1.0, 1.0, 0.0, 0.0, 1.0, 1.0,

            # Верхняя грань (y = 1)
            -1.0, 1.0, -1.0, 0.0, 1.0, 0.0, 0.0, 1.0,
            1.0, 1.0, -1.0, 0.0, 1.0, 0.0, 1.0, 1.0,
            1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
            -1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0,

            # Нижняя грань (y = -1)
            -1.0, -1.0, -1.0, 0.0, -1.0, 0.0, 0.0, 0.0,
            1.0, -1.0, -1.0, 0.0, -1.0, 0.0, 1.0, 0.0,
            1.0, -1.0, 1.0, 0.0, -1.0, 0.0, 1.0, 1.0,
            -1.0, -1.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0,
        ]

        # Индексы треугольников
        indices = [
            0, 1, 2, 0, 2, 3,  # передняя
            4, 5, 6, 4, 6, 7,  # задняя
            8, 9, 10, 8, 10, 11,  # левая
            12, 13, 14, 12, 14, 15,  # правая
            16, 17, 18, 16, 18, 19,  # верхняя
            20, 21, 22, 20, 22, 23  # нижняя
        ]

        # Загружаем вершины и индексы в меш
        mesh.set_vertices(vertices)
        mesh.set_indices(indices)

        # Создаем материал
        material = alkash3d.PBRMaterial()
        material.set_base_color(alkash3d.Vec4(1.0, 0.5, 0.0, 1.0))  # Оранжевый
        material.set_metallic(0.1)
        material.set_roughness(0.3)

        # Применяем материал к мешу
        mesh.set_material(material)

        # Добавляем меш к узлу
        cube_node.add_component(mesh)

        return cube_node

    def run(self):
        print("Запуск приложения...")
        clock = alkash3d.utils.Clock()

        while not self.window.should_close():
            dt = clock.tick()

            # Обработка событий окна
            self.window.poll_events()

            # Вращаем куб
            self.rotation += 50 * dt  # 50 градусов в секунду
            self.cube.set_rotation(alkash3d.Vec3(0, self.rotation, 0))

            # Рендеринг
            self.engine.clear(alkash3d.Vec4(0.1, 0.1, 0.1, 1.0))
            self.engine.render_scene(self.scene, self.camera)
            self.engine.present()

        self.engine.shutdown()
        print("Приложение завершено")


if __name__ == "__main__":
    app = CubeApp()
    app.run()