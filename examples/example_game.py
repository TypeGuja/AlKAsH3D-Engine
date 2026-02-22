#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
dx12_game_fixed.py - ИСПРАВЛЕННАЯ игра на чистом DX12 движке
"""

import sys
import math
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import glfw
import numpy as np

from alkash3d import Engine, Scene, Node, Vec3
from alkash3d.scene.light import DirectionalLight
from alkash3d.assets.material import PBRMaterial
from alkash3d.scene.mesh import Mesh


# ============================================================================
# Простой куб
# ============================================================================
class SimpleCube(Node):
    def __init__(self, color=(1.0, 1.0, 1.0, 1.0), size=1.0):
        super().__init__("Cube")

        # Вершины
        s = size
        vertices = np.array([
            [-s, -s, s], [s, -s, s], [s, s, s], [-s, s, s],
            [-s, -s, -s], [s, -s, -s], [s, s, -s], [-s, s, -s]
        ], dtype=np.float32).reshape(-1, 3)

        # Индексы
        indices = np.array([
            0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 0, 4, 7, 0, 7, 3,
            1, 5, 6, 1, 6, 2, 3, 2, 6, 3, 6, 7, 0, 1, 5, 0, 5, 4
        ], dtype=np.uint32)

        self.mesh = Mesh(vertices, indices=indices)
        self.material = PBRMaterial(albedo=color)
        self.mesh.material = self.material
        self.add_child(self.mesh)

    def draw(self, backend):
        if self.mesh:
            if self.material:
                self.material.bind(backend)
            self.mesh.draw(backend)


# ============================================================================
# ИСПРАВЛЕННЫЙ ForwardRenderer
# ============================================================================
class FixedForwardRenderer:
    """Исправленная версия ForwardRenderer без ошибок с параметрами."""

    def __init__(self, window, backend):
        self.window = window
        self.backend = backend
        self.width = window.width
        self.height = window.height

        # Белая текстура-заглушка (исправленные параметры)
        white_pixel = (255).to_bytes(1, "little") * 4
        self.white_tex = backend.create_texture(
            data=white_pixel,
            width=1,  # было 'w', теперь 'width'
            height=1,  # было 'h', теперь 'height'
            fmt="RGBA8"
        )

        # Шейдер (упрощенный)
        from alkash3d.renderer.shader import Shader
        self.shader = Shader(
            vertex_path=str(window.resource_path("shaders/forward_vert.hlsl")),
            fragment_path=str(window.resource_path("shaders/forward_frag.hlsl")),
            backend=backend
        )

        # Дескрипторы
        backend.set_descriptor_heaps([backend.rtv_heap, backend.cbv_srv_uav_heap])
        backend.set_graphics_pipeline(self.shader.pso)

    def resize(self, w, h):
        self.backend.set_viewport(0, 0, w, h)
        self.backend.set_scissor_rect(0, 0, w, h)

    def render(self, scene, camera):
        try:
            self.backend.begin_frame()

            # Очистка
            back_rtv = self.backend.rtv_heap.get_cpu_handle(0)
            self.backend.set_render_target(back_rtv)
            self.backend.clear_render_target(back_rtv, (0.1, 0.1, 0.2, 1.0))

            # Рендеринг сцены
            self.shader.use()
            self.shader.set_uniform_mat4("uView", camera.get_view_matrix())
            self.shader.set_uniform_mat4("uProj",
                                         camera.get_projection_matrix(self.width / self.height))

            # Проходим по всем объектам
            for node in scene.traverse():
                if hasattr(node, "draw") and getattr(node, "visible", True):
                    model = node.get_world_matrix().to_gl()
                    self.shader.set_uniform_mat4("uModel", model)

                    if hasattr(node, "material") and node.material:
                        node.material.bind(self.backend)

                    node.draw(self.backend)

            self.shader.flush()
            self.backend.end_frame()

        except Exception as e:
            print(f"Render error: {e}")


# ============================================================================
# ИСПРАВЛЕННЫЙ Engine
# ============================================================================
class FixedEngine:
    """Исправленная версия Engine."""

    def __init__(self, width=1024, height=768, title="Game"):
        self.width = width
        self.height = height

        # Конфиг
        from alkash3d.utils.config import Config
        self.cfg = Config()

        # Окно
        from alkash3d.window import Window
        self.window = Window(width, height, title)

        # Бэкенд
        from alkash3d.graphics import select_backend
        self.backend = select_backend("dx12")
        self.backend.init_device(self.window.hwnd, width, height)
        self.window.backend = self.backend

        # Камера
        from alkash3d.scene.camera import Camera
        self.camera = Camera()

        # Таймер
        from alkash3d.utils.timer import Timer
        self.timer = Timer()

        # Рендерер (исправленный)
        self.renderer = FixedForwardRenderer(self.window, self.backend)

    def shutdown(self):
        self.backend.shutdown()
        self.window.close()


# ============================================================================
# Игровая сцена
# ============================================================================
class GameScene(Scene):
    def __init__(self):
        super().__init__()

        print("🟢 Создание сцены...")

        # ПОЛ
        self.floor = SimpleCube(color=(0.3, 0.3, 0.3, 1.0), size=20)
        self.floor.position = Vec3(0, -1, 0)
        self.add_child(self.floor)

        # ИГРОК (красный)
        self.player = SimpleCube(color=(1.0, 0.2, 0.2, 1.0))
        self.player.position = Vec3(0, 1, 0)
        self.add_child(self.player)

        # ЦЕЛЬ (зеленый)
        self.target = SimpleCube(color=(0.2, 1.0, 0.2, 1.0))
        self.target.position = Vec3(5, 1, 5)
        self.add_child(self.target)

        # ОСВЕЩЕНИЕ
        self.sun = DirectionalLight(
            direction=Vec3(0.5, -1.0, 0.5),
            intensity=1.5
        )
        self.add_child(self.sun)

        self.game_won = False
        self.target_hit = False
        print("✅ Сцена готова!")

    def update(self, dt, input_mgr):
        if self.game_won:
            return self.player.position

        # Движение
        speed = 5.0 * dt
        if input_mgr.is_key_pressed(glfw.KEY_W):
            self.player.position.z -= speed
        if input_mgr.is_key_pressed(glfw.KEY_S):
            self.player.position.z += speed
        if input_mgr.is_key_pressed(glfw.KEY_A):
            self.player.position.x -= speed
        if input_mgr.is_key_pressed(glfw.KEY_D):
            self.player.position.x += speed

        # Вращение цели
        self.target.rotation.y += 90 * dt

        # Проверка победы
        if not self.target_hit:
            dx = self.player.position.x - self.target.position.x
            dz = self.player.position.z - self.target.position.z
            if math.sqrt(dx * dx + dz * dz) < 2.0:
                self.target.visible = False
                self.target_hit = True
                self.game_won = True
                print("\n🎉 ПОБЕДА!")

        return self.player.position


# ============================================================================
# Главный класс игры
# ============================================================================
class DX12Game:
    def __init__(self):
        print("\n" + "=" * 60)
        print("🎮 DX12 GAME - ИСПРАВЛЕННАЯ ВЕРСИЯ")
        print("=" * 60)
        print("Управление: WASD - движение")
        print("           ESC - выход")
        print("=" * 60 + "\n")

        try:
            # Исправленный движок
            self.engine = FixedEngine(1024, 768, "DX12 Game")
            self.scene = GameScene()
            self.engine.scene = self.scene

            # Камера от третьего лица
            self.camera_offset = Vec3(15, 8, 15)

            print("✅ ДВИЖОК ЗАПУЩЕН!")
            print("🎮 ИГРА НАЧИНАЕТСЯ...\n")

        except Exception as e:
            print(f"\n❌ ОШИБКА: {e}")
            raise

    def run(self):
        """Главный цикл."""
        last_fps_time = time.time()
        frames = 0

        while not self.engine.window.should_close():
            dt = self.engine.timer.tick()

            # Обновление игры
            player_pos = self.scene.update(dt, self.engine.window.input)

            # Камера следует за игроком
            self.engine.camera.position = Vec3(
                player_pos.x + self.camera_offset.x,
                player_pos.y + self.camera_offset.y,
                player_pos.z + self.camera_offset.z
            )

            # РЕНДЕРИНГ
            self.engine.renderer.render(self.engine.scene, self.engine.camera)

            # События
            self.engine.window.poll_events()

            # FPS счетчик
            frames += 1
            now = time.time()
            if now - last_fps_time >= 1.0:
                fps = frames
                frames = 0
                last_fps_time = now

                pos = self.scene.player.position
                status = f"\rFPS: {fps} | Позиция: ({pos.x:.1f}, {pos.z:.1f})"
                if self.scene.game_won:
                    status += " | 🏆 ПОБЕДА!"
                print(status, end="")

            # Выход по ESC
            if self.engine.window.input.is_key_pressed(glfw.KEY_ESCAPE):
                break

        self.engine.shutdown()
        print("\n\n👋 Игра завершена!")


# ============================================================================
# Запуск
# ============================================================================
if __name__ == "__main__":
    try:
        game = DX12Game()
        game.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Прервано")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback

        traceback.print_exc()