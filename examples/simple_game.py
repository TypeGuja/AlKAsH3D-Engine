#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
3D GAME - Настоящая 3D игра на DX12!
Управление: WASD - движение, Space - огонь, ESC - выход
"""

import sys
import time
import math
import random
import struct
from pathlib import Path
from dataclasses import dataclass
from typing import List, Tuple

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import glfw
from alkash3d.graphics import select_backend
import numpy as np


@dataclass
class Vector3:
    x: float = 0.0
    y: float = 0.0
    z: float = 0.0


@dataclass
class Player:
    pos: Vector3
    rotation: float = 0.0
    speed: float = 0.1
    color: Tuple[float, float, float, float] = (0.2, 0.6, 1.0, 1.0)


@dataclass
class Bullet:
    pos: Vector3
    dir: Vector3
    speed: float = 0.2
    life: float = 3.0
    color: Tuple[float, float, float, float] = (1.0, 1.0, 0.0, 1.0)


@dataclass
class Enemy:
    pos: Vector3
    type: int = 0  # 0 = куб, 1 = пирамида
    hp: int = 3
    speed: float = 0.05
    color: Tuple[float, float, float, float] = (1.0, 0.2, 0.2, 1.0)


class Game3D:
    def __init__(self):
        print("\n" + "=" * 60)
        print("🎮 3D GAME - НАСТОЯЩАЯ 3D ИГРА НА DX12!")
        print("=" * 60)
        print("Управление:")
        print("  WASD - движение вперед/назад/влево/вправо")
        print("  Q/E - поворот камеры")
        print("  Space - стрельба")
        print("  ESC - выход")
        print("=" * 60 + "\n")

        # Инициализация GLFW
        if not glfw.init():
            raise RuntimeError("Failed to init GLFW")

        # Создаем окно
        glfw.window_hint(glfw.CLIENT_API, glfw.NO_API)
        self.window = glfw.create_window(1280, 720, "3D GAME", None, None)
        if not self.window:
            glfw.terminate()
            raise RuntimeError("Failed to create window")

        # Настройка обработки клавиш
        glfw.set_key_callback(self.window, self.key_callback)

        self.hwnd = glfw.get_win32_window(self.window)
        self.backend = select_backend("dx12")

        # Игровые объекты
        self.player = Player(pos=Vector3(0, 1, -5))
        self.bullets: List[Bullet] = []
        self.enemies: List[Enemy] = []
        self.score = 0
        self.game_time = 0
        self.last_shot = 0
        self.shoot_cooldown = 0.3

        # Состояние клавиш
        self.keys = {
            'w': False, 'a': False, 's': False, 'd': False,
            'q': False, 'e': False, 'space': False
        }

        # Матрицы для 3D
        self.projection_matrix = self.create_projection_matrix(
            70.0, 1280 / 720, 0.1, 100.0
        )
        self.view_matrix = self.create_view_matrix(
            self.player.pos, self.player.rotation
        )

        # Вершинные данные для куба
        self.cube_vertices = self.create_cube_vertices()
        self.cube_indices = self.create_cube_indices()

        # Вершинные данные для пирамиды
        self.pyramid_vertices = self.create_pyramid_vertices()
        self.pyramid_indices = self.create_pyramid_indices()

        # Статистика
        self.fps = 0
        self.frame_count = 0
        self.fps_time = time.time()
        self.device_ok = False

        # Цвет неба (меняется в зависимости от времени)
        self.sky_color = (0.5, 0.7, 1.0, 1.0)

    def create_projection_matrix(self, fov: float, aspect: float, near: float, far: float) -> List[float]:
        """Создает матрицу проекции"""
        f = 1.0 / math.tan(math.radians(fov) / 2.0)
        return [
            f / aspect, 0, 0, 0,
            0, f, 0, 0,
            0, 0, (far + near) / (near - far), -1,
            0, 0, (2 * far * near) / (near - far), 0
        ]

    def create_view_matrix(self, pos: Vector3, rot: float) -> List[float]:
        """Создает матрицу вида"""
        # Упрощенная матрица вида
        cos_r = math.cos(rot)
        sin_r = math.sin(rot)
        return [
            cos_r, 0, sin_r, 0,
            0, 1, 0, 0,
            -sin_r, 0, cos_r, 0,
            -pos.x * cos_r + pos.z * sin_r, -pos.y, -pos.x * sin_r - pos.z * cos_r, 1
        ]

    def create_cube_vertices(self) -> List[float]:
        """Создает вершины куба (позиция + нормаль + цвет)"""
        s = 0.5  # половина размера
        vertices = []

        #           position        normal         color
        # Передняя грань
        vertices.extend([-s, -s, s, 0, 0, 1, 1, 0, 0])  # красный
        vertices.extend([s, -s, s, 0, 0, 1, 1, 0, 0])
        vertices.extend([s, s, s, 0, 0, 1, 1, 0, 0])
        vertices.extend([-s, s, s, 0, 0, 1, 1, 0, 0])

        # Задняя грань
        vertices.extend([-s, -s, -s, 0, 0, -1, 0, 1, 0])  # зеленый
        vertices.extend([-s, s, -s, 0, 0, -1, 0, 1, 0])
        vertices.extend([s, s, -s, 0, 0, -1, 0, 1, 0])
        vertices.extend([s, -s, -s, 0, 0, -1, 0, 1, 0])

        # Левая грань
        vertices.extend([-s, -s, -s, -1, 0, 0, 0, 0, 1])  # синий
        vertices.extend([-s, -s, s, -1, 0, 0, 0, 0, 1])
        vertices.extend([-s, s, s, -1, 0, 0, 0, 0, 1])
        vertices.extend([-s, s, -s, -1, 0, 0, 0, 0, 1])

        # Правая грань
        vertices.extend([s, -s, -s, 1, 0, 0, 1, 1, 0])  # желтый
        vertices.extend([s, s, -s, 1, 0, 0, 1, 1, 0])
        vertices.extend([s, s, s, 1, 0, 0, 1, 1, 0])
        vertices.extend([s, -s, s, 1, 0, 0, 1, 1, 0])

        # Верхняя грань
        vertices.extend([-s, s, -s, 0, 1, 0, 1, 0, 1])  # пурпурный
        vertices.extend([-s, s, s, 0, 1, 0, 1, 0, 1])
        vertices.extend([s, s, s, 0, 1, 0, 1, 0, 1])
        vertices.extend([s, s, -s, 0, 1, 0, 1, 0, 1])

        # Нижняя грань
        vertices.extend([-s, -s, -s, 0, -1, 0, 0, 1, 1])  # циан
        vertices.extend([s, -s, -s, 0, -1, 0, 0, 1, 1])
        vertices.extend([s, -s, s, 0, -1, 0, 0, 1, 1])
        vertices.extend([-s, -s, s, 0, -1, 0, 0, 1, 1])

        return vertices

    def create_cube_indices(self) -> List[int]:
        """Создает индексы для куба"""
        indices = []
        for i in range(6):  # 6 граней
            base = i * 4
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3])
        return indices

    def create_pyramid_vertices(self) -> List[float]:
        """Создает вершины пирамиды"""
        h = 0.7  # высота
        r = 0.5  # радиус основания
        vertices = []

        # Основание (квадрат)
        vertices.extend([-r, -h / 2, -r, 0, -1, 0, 0.8, 0.8, 0.8])
        vertices.extend([r, -h / 2, -r, 0, -1, 0, 0.8, 0.8, 0.8])
        vertices.extend([r, -h / 2, r, 0, -1, 0, 0.8, 0.8, 0.8])
        vertices.extend([-r, -h / 2, r, 0, -1, 0, 0.8, 0.8, 0.8])

        # Грани
        vertices.extend([-r, -h / 2, -r, -0.5, 0.5, -0.5, 1, 0, 0])  # красный
        vertices.extend([r, -h / 2, -r, 0.5, 0.5, -0.5, 0, 1, 0])  # зеленый
        vertices.extend([r, -h / 2, r, 0.5, 0.5, 0.5, 0, 0, 1])  # синий
        vertices.extend([-r, -h / 2, r, -0.5, 0.5, 0.5, 1, 1, 0])  # желтый
        vertices.extend([0, h / 2, 0, 0, 1, 0, 1, 1, 1])  # вершина (белая)

        return vertices

    def create_pyramid_indices(self) -> List[int]:
        """Создает индексы для пирамиды"""
        return [
            # Основание
            0, 1, 2, 0, 2, 3,
            # Грани
            0, 4, 1,
            1, 4, 2,
            2, 4, 3,
            3, 4, 0
        ]

    def key_callback(self, window, key, scancode, action, mods):
        """Обработка нажатий клавиш"""
        if key == glfw.KEY_ESCAPE and action == glfw.PRESS:
            glfw.set_window_should_close(window, True)

        # Движение
        if key == glfw.KEY_W:
            self.keys['w'] = (action == glfw.PRESS or action == glfw.REPEAT)
        if key == glfw.KEY_S:
            self.keys['s'] = (action == glfw.PRESS or action == glfw.REPEAT)
        if key == glfw.KEY_A:
            self.keys['a'] = (action == glfw.PRESS or action == glfw.REPEAT)
        if key == glfw.KEY_D:
            self.keys['d'] = (action == glfw.PRESS or action == glfw.REPEAT)
        if key == glfw.KEY_Q:
            self.keys['q'] = (action == glfw.PRESS or action == glfw.REPEAT)
        if key == glfw.KEY_E:
            self.keys['e'] = (action == glfw.PRESS or action == glfw.REPEAT)

        # Стрельба
        if key == glfw.KEY_SPACE:
            self.keys['space'] = (action == glfw.PRESS or action == glfw.REPEAT)

    def init_device(self):
        """Инициализация устройства"""
        try:
            print("⏳ Инициализация DX12...")
            self.backend.init_device(self.hwnd, 1280, 720)

            if not self.backend.swap_chain:
                print("❌ Swap chain не создан!")
                return False

            print(f"✅ Swap chain: {hex(self.backend.swap_chain.value)}")
            print(f"✅ RTV дескрипторов: {len(self.backend._rtv_cpu_handles)}")

            # TODO: Создать шейдеры и PSO для 3D рендеринга
            # Пока просто чистим экран

            self.device_ok = True
            return True

        except Exception as e:
            print(f"❌ Ошибка: {e}")
            return False

    def update_camera(self):
        """Обновляет камеру на основе ввода"""
        speed = self.player.speed

        # Поворот
        if self.keys['q']:
            self.player.rotation += 0.05
        if self.keys['e']:
            self.player.rotation -= 0.05

        # Движение вперед/назад
        if self.keys['w']:
            self.player.pos.x += math.sin(self.player.rotation) * speed
            self.player.pos.z += math.cos(self.player.rotation) * speed
        if self.keys['s']:
            self.player.pos.x -= math.sin(self.player.rotation) * speed
            self.player.pos.z -= math.cos(self.player.rotation) * speed

        # Движение влево/вправо (strafe)
        if self.keys['a']:
            self.player.pos.x -= math.cos(self.player.rotation) * speed
            self.player.pos.z += math.sin(self.player.rotation) * speed
        if self.keys['d']:
            self.player.pos.x += math.cos(self.player.rotation) * speed
            self.player.pos.z -= math.sin(self.player.rotation) * speed

        # Обновляем матрицу вида
        self.view_matrix = self.create_view_matrix(self.player.pos, self.player.rotation)

    def spawn_enemy(self):
        """Создает врага в случайном месте"""
        angle = random.uniform(0, math.pi * 2)
        distance = random.uniform(5, 15)
        x = math.sin(angle) * distance
        z = math.cos(angle) * distance
        y = random.uniform(0.5, 2.0)

        self.enemies.append(Enemy(
            pos=Vector3(x, y, z),
            type=random.randint(0, 1),
            hp=random.randint(1, 3)
        ))

    def update(self, dt: float):
        """Обновление игровой логики"""
        self.game_time += dt
        self.update_camera()

        # Стрельба
        if self.keys['space'] and self.game_time - self.last_shot > self.shoot_cooldown:
            self.last_shot = self.game_time
            # Создаем пулю в направлении камеры
            bullet = Bullet(
                pos=Vector3(self.player.pos.x, self.player.pos.y, self.player.pos.z),
                dir=Vector3(
                    math.sin(self.player.rotation),
                    0,
                    math.cos(self.player.rotation)
                )
            )
            self.bullets.append(bullet)

        # Обновление пуль
        for bullet in self.bullets[:]:
            bullet.pos.x += bullet.dir.x * bullet.speed
            bullet.pos.y += bullet.dir.y * bullet.speed
            bullet.pos.z += bullet.dir.z * bullet.speed
            bullet.life -= dt

            # Проверка попаданий
            for enemy in self.enemies[:]:
                dx = bullet.pos.x - enemy.pos.x
                dy = bullet.pos.y - enemy.pos.y
                dz = bullet.pos.z - enemy.pos.z
                dist = math.sqrt(dx * dx + dy * dy + dz * dz)
                if dist < 1.0:
                    enemy.hp -= 1
                    if enemy.hp <= 0:
                        self.enemies.remove(enemy)
                        self.score += 100
                    if bullet in self.bullets:
                        self.bullets.remove(bullet)
                    break

            # Удаление старых пуль
            if bullet.life <= 0:
                self.bullets.remove(bullet)

        # Спавн врагов
        if len(self.enemies) < 5 and random.random() < 0.01:
            self.spawn_enemy()

    def render_frame(self):
        """Отрисовка кадра"""
        if not self.device_ok:
            return False

        try:
            # Начало кадра
            if not self.backend.begin_frame():
                self.device_ok = False
                return False

            # Получаем текущий back buffer
            frame_index = self.backend.get_frame_index()
            if hasattr(self.backend, '_rtv_cpu_handles') and len(self.backend._rtv_cpu_handles) > frame_index:
                rtv = self.backend._rtv_cpu_handles[frame_index]
            else:
                rtv = self.backend.rtv_heap.get_cpu_handle(0)

            # Устанавливаем render target
            self.backend.set_render_target(rtv)

            # Цвет неба меняется со временем
            t = self.game_time * 0.1
            self.sky_color = (
                0.5 + 0.2 * math.sin(t),
                0.6 + 0.2 * math.sin(t + 2),
                1.0,
                1.0
            )

            # Очищаем экран цветом неба
            self.backend.clear_render_target(rtv, self.sky_color)

            # Завершаем кадр
            if not self.backend.end_frame():
                return False

            # Презентуем
            if not self.backend.present(sync_interval=1):
                self.device_ok = False
                return False

            return True

        except Exception as e:
            print(f"❌ Ошибка рендеринга: {e}")
            self.device_ok = False
            return False

    def run(self):
        """Главный игровой цикл"""
        if not self.init_device():
            return

        print("\n" + "=" * 60)
        print("🟢 3D ИГРА ЗАПУЩЕНА!")
        print("=" * 60 + "\n")
        print("🌍 Исследуйте 3D мир!")
        print("   WASD - движение")
        print("   Q/E - поворот камеры")
        print("   Space - стрельба")
        print("   ESC - выход\n")

        last_time = time.time()

        # Таймеры для спавна
        spawn_timer = 0

        while not glfw.window_should_close(self.window):
            current_time = time.time()
            dt = current_time - last_time
            last_time = current_time

            # Ограничение FPS
            if dt > 0.033:
                dt = 0.033

            # Обработка событий
            glfw.poll_events()

            # Обновление игры
            self.update(dt)

            # Спавн врагов по таймеру
            spawn_timer += dt
            if spawn_timer > 3.0:
                spawn_timer = 0
                self.spawn_enemy()

            # Рендеринг
            if not self.render_frame():
                print("\n❌ Ошибка рендеринга")
                time.sleep(0.5)
                continue

            # Статистика
            self.frame_count += 1
            if current_time - self.fps_time >= 1.0:
                self.fps = self.frame_count
                self.frame_count = 0
                self.fps_time = current_time

                # Статистика в консоль
                sys.stdout.write(
                    f"\r🎯 Счет: {self.score}  |  👾 Врагов: {len(self.enemies)}  |  🔫 Пуль: {len(self.bullets)}  |  FPS: {self.fps}  |  Позиция: ({self.player.pos.x:.1f}, {self.player.pos.y:.1f}, {self.player.pos.z:.1f})")
                sys.stdout.flush()

        self.cleanup()

    def cleanup(self):
        """Очистка"""
        print("\n\n👋 Завершение игры...")
        self.backend.shutdown()
        glfw.terminate()
        print(f"🎮 Итоговый счет: {self.score}")
        print("✅ Игра завершена")


if __name__ == "__main__":
    try:
        game = Game3D()
        game.run()
    except KeyboardInterrupt:
        print("\n\n⚠️ Игра прервана")
    except Exception as e:
        print(f"\n❌ Ошибка: {e}")
        import traceback

        traceback.print_exc()