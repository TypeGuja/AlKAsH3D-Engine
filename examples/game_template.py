# examples/game_template.py
import alkash3d as ak
import numpy as np


class MyGame:
    def __init__(self):
        self.engine = ak.Engine(
            width=1280,
            height=720,
            title="My AlKAsH3D Game"
        )

        self.rotation_speed = 50.0
        self.player_position = ak.Vec3(0, 0, 0)

        self.setup_game()

    def setup_game(self):
        """Настройка игровой сцены"""
        self.create_environment()
        self.create_lights()

        # Настройка камеры
        self.engine.camera.position = ak.Vec3(0, 2, 8)

    def create_environment(self):
        """Создание окружения"""
        # Создаем пол
        floor_vertices = np.array([
            -10.0, -1.0, -10.0, 10.0, -1.0, -10.0, 10.0, -1.0, 10.0,
            -10.0, -1.0, -10.0, 10.0, -1.0, 10.0, -10.0, -1.0, 10.0,
        ], dtype=np.float32)

        floor = ak.Mesh(vertices=floor_vertices, name="Floor")
        floor.position = ak.Vec3(0, -2, 0)
        self.engine.scene.add_child(floor)

        # Создаем куб (добавляем атрибут cube)
        cube_vertices = np.array([
            # Передняя грань
            -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0,
            -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0,
            # Задняя грань
            -1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0,
            -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0,
        ], dtype=np.float32)

        self.cube = ak.Mesh(vertices=cube_vertices, name="Cube")
        self.cube.position = ak.Vec3(0, 0, 0)
        self.engine.scene.add_child(self.cube)

    def create_lights(self):
        """Создание освещения"""
        main_light = ak.DirectionalLight(
            direction=ak.Vec3(-0.5, -1, -0.5),
            color=ak.Vec3(1, 1, 0.9),
            intensity=0.8
        )
        self.engine.scene.add_child(main_light)

        fill_light = ak.DirectionalLight(
            direction=ak.Vec3(0.5, -0.3, 0.5),
            color=ak.Vec3(0.3, 0.3, 0.5),
            intensity=0.3
        )
        self.engine.scene.add_child(fill_light)

    def update(self, dt):
        """Обновление игровой логики"""
        # Вращение куба
        if hasattr(self, 'cube'):
            self.cube.rotation.y += self.rotation_speed * dt

    def run(self):
        """Запуск игрового цикла"""
        import time

        last_time = time.time()
        while not self.engine.window.should_close():
            current_time = time.time()
            dt = current_time - last_time
            last_time = current_time

            self.engine.window.poll_events()
            self.engine.camera.update_fly(dt, self.engine.window.input)

            self.update(dt)
            self.engine.scene.update(dt)

            self.engine.renderer.render(self.engine.scene, self.engine.camera)
            self.engine.window.swap_buffers()

        self.engine.shutdown()


if __name__ == "__main__":
    game = MyGame()
    game.run()