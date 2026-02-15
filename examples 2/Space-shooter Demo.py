#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Space‑Shooter demo for AlKAsH3D (исправленная версия).

* Красный корабль (WASD + Space – стрельба).
* Пули (желтые кубики) и астероиды (серые кубы).
* UI‑overlay – полоска жизней в левом‑нижнем углу.
* Опциональный звук (pygame.mixer) – выстрел и взрыв.
* Камера следует за кораблём (first‑person).
* Перезапуск уровня клавишей R.
"""

# ----------------------------------------------------------------------
# 0️⃣  Imports (public API + optional sound + typing)
# ----------------------------------------------------------------------
import alkash3d as ak                     # Engine, Scene, Camera, Mesh, …
from alkash3d.renderer import ForwardRenderer   # forward‑pipeline
import numpy as np
import glfw
import math
import random
from OpenGL import GL                    # нужен только для BLEND и UI‑рисования
from typing import List                # типовые аннотации

# optional sound (pygame). Если нет – игра без звука.
try:
    import pygame.mixer as mixer
    _SOUND_AVAILABLE = True
except Exception:
    _SOUND_AVAILABLE = False


# ----------------------------------------------------------------------
# 1️⃣  InlineShader (UI‑overlay)
# ----------------------------------------------------------------------
class InlineShader:
    """Компилирует простой vertex+fragment‑шейдер из строк (GLSL 330 core)."""
    def __init__(self, vert_src: str, frag_src: str):
        self.vert_src = vert_src
        self.frag_src = frag_src
        self.program = None
        self.compile()

    def compile(self):
        # vertex
        vert = GL.glCreateShader(GL.GL_VERTEX_SHADER)
        GL.glShaderSource(vert, self.vert_src)
        GL.glCompileShader(vert)
        self._check_compile(vert, "VERTEX")

        # fragment
        frag = GL.glCreateShader(GL.GL_FRAGMENT_SHADER)
        GL.glShaderSource(frag, self.frag_src)
        GL.glCompileShader(frag)
        self._check_compile(frag, "FRAGMENT")

        # link
        prog = GL.glCreateProgram()
        GL.glAttachShader(prog, vert)
        GL.glAttachShader(prog, frag)
        GL.glLinkProgram(prog)
        self._check_link(prog)

        GL.glDeleteShader(vert)
        GL.glDeleteShader(frag)

        self.program = prog

    # ------------------------------------------------------------------
    def _check_compile(self, shader, typ):
        status = GL.glGetShaderiv(shader, GL.GL_COMPILE_STATUS)
        if not status:
            log = GL.glGetShaderInfoLog(shader).decode()
            raise RuntimeError(f"{typ} compile error:\n{log}")

    def _check_link(self, program):
        status = GL.glGetProgramiv(program, GL.GL_LINK_STATUS)
        if not status:
            log = GL.glGetProgramInfoLog(program).decode()
            raise RuntimeError(f"Program link error:\n{log}")

    # ------------------------------------------------------------------
    def use(self):
        if self.program:
            GL.glUseProgram(self.program)

    def set_uniform_vec3(self, name: str, vec):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        data = vec.as_np() if hasattr(vec, "as_np") else np.array(vec, dtype=np.float32)
        GL.glUniform3fv(loc, 1, data)


# ----------------------------------------------------------------------
# 2️⃣  Primitive (cube mesh)
# ----------------------------------------------------------------------
def make_cube_mesh(name: str = "Box") -> ak.Mesh:
    """Куб (24 вершины, нормали, индексы) – подходит под fallback‑shader."""
    pos = np.array([
        # Front (+Z)
        -0.5, -0.5,  0.5,
         0.5, -0.5,  0.5,
         0.5,  0.5,  0.5,
        -0.5,  0.5,  0.5,
        # Back (-Z)
        -0.5, -0.5, -0.5,
        -0.5,  0.5, -0.5,
         0.5,  0.5, -0.5,
         0.5, -0.5, -0.5,
        # Left (-X)
        -0.5, -0.5, -0.5,
        -0.5, -0.5,  0.5,
        -0.5,  0.5,  0.5,
        -0.5,  0.5, -0.5,
        # Right (+X)
         0.5, -0.5, -0.5,
         0.5,  0.5, -0.5,
         0.5,  0.5,  0.5,
         0.5, -0.5,  0.5,
        # Top (+Y)
        -0.5,  0.5,  0.5,
         0.5,  0.5,  0.5,
         0.5,  0.5, -0.5,
        -0.5,  0.5, -0.5,
        # Bottom (-Y)
        -0.5, -0.5,  0.5,
        -0.5, -0.5, -0.5,
         0.5, -0.5, -0.5,
         0.5, -0.5,  0.5,
    ], dtype=np.float32)

    norm = np.array([
        # Front
        0,0,1, 0,0,1, 0,0,1, 0,0,1,
        # Back
        0,0,-1, 0,0,-1, 0,0,-1, 0,0,-1,
        # Left
        -1,0,0, -1,0,0, -1,0,0, -1,0,0,
        # Right
        1,0,0, 1,0,0, 1,0,0, 1,0,0,
        # Top
        0,1,0, 0,1,0, 0,1,0, 0,1,0,
        # Bottom
        0,-1,0, 0,-1,0, 0,-1,0, 0,-1,0,
    ], dtype=np.float32)

    idx = np.array([
        0,1,2, 0,2,3,       # front
        4,5,6, 4,6,7,       # back
        8,9,10,8,10,11,    # left
        12,13,14,12,14,15, # right
        16,17,18,16,18,19, # top
        20,21,22,20,22,23, # bottom
    ], dtype=np.uint32)

    return ak.Mesh(vertices=pos, normals=norm, indices=idx, name=name)


# ----------------------------------------------------------------------
# 3️⃣  Игровые сущности: Ship, Bullet, Asteroid
# ----------------------------------------------------------------------
class Ship(ak.Node):
    """Игрок‑корабль (красный куб)."""
    SPEED = 12.0          # м/с
    SHOOT_COOLDOWN = 0.25 # сек

    def __init__(self):
        super().__init__("Ship")
        body = make_cube_mesh()
        body.scale = ak.Vec3(1.2, 0.5, 2.0)
        body.color = ak.Vec3(0.9, 0.2, 0.2)
        self.add_child(body)

        self.input = None
        self.shoot_timer = 0.0
        self.scene_root = None            # будет назначено из main()
        self.forward = ak.Vec3(0,0,1)
        self.right   = ak.Vec3(1,0,0)

        # звук выстрела (если pygame)
        self.shoot_chan = None
        if _SOUND_AVAILABLE:
            try:
                mixer.init()
                self.shoot_snd = mixer.Sound(buffer=b'\x00'*1024)
                self.shoot_chan = self.shoot_snd.play(loops=0)
            except Exception:
                self.shoot_chan = None

    def bind_input(self, im):
        self.input = im

    def _shoot(self):
        bullet = Bullet()
        bullet.position = self.position + self.forward * 1.2
        bullet.velocity = self.forward * Bullet.SPEED
        if self.scene_root:
            self.scene_root.add_child(bullet)

        if self.shoot_chan:
            self.shoot_chan.set_volume(0.5)

    def on_update(self, dt: float):
        if self.input is None:
            return

        # ---------- движение ----------
        move = ak.Vec3()
        if self.input.is_key_pressed(glfw.KEY_W):
            move += self.forward
        if self.input.is_key_pressed(glfw.KEY_S):
            move -= self.forward
        if self.input.is_key_pressed(glfw.KEY_A):
            move -= self.right
        if self.input.is_key_pressed(glfw.KEY_D):
            move += self.right

        if move.length() > 0.0:
            move = move.normalized() * self.SPEED * dt
            self.position = self.position + move

        # ---------- вращение мышью ----------
        dx, dy = self.input.get_mouse_delta()
        self.rotation.y += dx * 0.1
        self.rotation.x = max(-30.0, min(30.0, self.rotation.x + dy * 0.1))

        # обновляем векторы forward/right
        yaw = math.radians(self.rotation.y)
        self.forward = ak.Vec3(math.sin(yaw), 0.0, math.cos(yaw))
        self.right   = ak.Vec3(math.sin(yaw + math.pi/2), 0.0, math.cos(yaw + math.pi/2))

        # ---------- стрельба ----------
        self.shoot_timer = max(0.0, self.shoot_timer - dt)
        if self.input.is_key_pressed(glfw.KEY_SPACE) and self.shoot_timer <= 0.0:
            self._shoot()
            self.shoot_timer = self.SHOOT_COOLDOWN


class Bullet(ak.Node):
    """Пуля (желтый маленький куб)."""
    SPEED = 30.0
    LIFETIME = 2.0

    def __init__(self):
        super().__init__("Bullet")
        mesh = make_cube_mesh()
        mesh.scale = ak.Vec3(0.15, 0.15, 0.3)
        mesh.color = ak.Vec3(0.9, 0.9, 0.2)
        self.add_child(mesh)

        self.velocity = ak.Vec3()
        self.age = 0.0

    def on_update(self, dt: float):
        self.position = self.position + self.velocity * dt
        self.age += dt


class Asteroid(ak.Node):
    """Астероид (серый куб)."""
    SPEED = 8.0

    def __init__(self, target: Ship):
        super().__init__("Asteroid")
        mesh = make_cube_mesh()
        scale = random.uniform(0.7, 1.5)
        mesh.scale = ak.Vec3(scale, scale, scale)
        mesh.color = ak.Vec3(0.6, 0.6, 0.6)
        self.add_child(mesh)

        # позиция в радиусе 15–30 м от центра, но в X‑Z‑плоскости
        angle = random.uniform(0, 2*math.pi)
        radius = random.uniform(15.0, 30.0)
        self.position = ak.Vec3(radius*math.cos(angle), 0.0, radius*math.sin(angle))

        # направление к цели + небольшое отклонение
        dir_vec = (target.position - self.position).normalized()
        jitter = ak.Vec3(random.uniform(-0.1, 0.1),
                         random.uniform(-0.05, 0.05),
                         random.uniform(-0.1, 0.1))
        self.velocity = (dir_vec + jitter).normalized() * self.SPEED
        self.radius = scale * 0.5   # для простой проверки коллизий


# ----------------------------------------------------------------------
# 4️⃣  UI‑Overlay (жизне‑полоса)
# ----------------------------------------------------------------------
UI_VERTEX_SRC = """#version 330 core
layout(location = 0) in vec2 aPos;   // NDC
void main() { gl_Position = vec4(aPos, 0.0, 1.txt.0); }
"""

UI_FRAGMENT_SRC = """#version 330 core
uniform vec3 uColor;
out vec4 FragColor;
void main() { FragColor = vec4(uColor, 1.txt.0); }
"""

class UIOverlay:
    """Рисует 2‑D прямоугольники (health‑bar)."""
    def __init__(self, width: int, height: int):
        self.shader = InlineShader(UI_VERTEX_SRC, UI_FRAGMENT_SRC)
        self._setup_vao()
        self.width, self.height = width, height

    def _setup_vao(self):
        self.vao = GL.glGenVertexArrays(1)
        GL.glBindVertexArray(self.vao)

        self.vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.vbo)
        GL.glBufferData(GL.GL_ARRAY_BUFFER, 8*4, None, GL.GL_DYNAMIC_DRAW)

        GL.glEnableVertexAttribArray(0)
        GL.glVertexAttribPointer(0, 2, GL.GL_FLOAT, GL.GL_FALSE, 0, None)

        GL.glBindVertexArray(0)

    # ------------------------------------------------------------------
    def _px_to_ndc(self, x_px: int, y_px: int):
        ndc_x = -1.0 + 2.0 * x_px / self.width
        ndc_y = -1.0 + 2.0 * y_px / self.height
        return ndc_x, ndc_y

    # ------------------------------------------------------------------
    def draw_rect(self, x_px: int, y_px: int,
                  w_px: int, h_px: int,
                  color: ak.Vec3):
        x0, y0 = self._px_to_ndc(x_px, y_px)
        x1 = x0 + 2.0 * w_px / self.width
        y1 = y0 + 2.0 * h_px / self.height

        verts = np.array([x0, y0,
                          x1, y0,
                          x1, y1,
                          x0, y1], dtype=np.float32)

        GL.glBindVertexArray(self.vao)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.vbo)
        GL.glBufferSubData(GL.GL_ARRAY_BUFFER, 0, verts.nbytes, verts)

        self.shader.use()
        self.shader.set_uniform_vec3("uColor", color)

        GL.glDrawArrays(GL.GL_TRIANGLE_FAN, 0, 4)
        GL.glBindVertexArray(0)

    # ------------------------------------------------------------------
    def resize(self, w: int, h: int):
        self.width, self.height = w, h


# ----------------------------------------------------------------------
# 5️⃣  UI‑Forward‑Renderer (draw UI after forward rendering)
# ----------------------------------------------------------------------
class UIForwardRenderer(ForwardRenderer):
    """
    Наследуем обычный ForwardRenderer и добавляем UI‑Overlay.
    Требуется ссылка на `ship` (жизни) и на `game_logic` (счёт, окно).
    """
    def __init__(self, window, ship: Ship, game_logic):
        super().__init__(window)
        self.ui = UIOverlay(window.width, window.height)
        self.ship = ship
        self.game_logic = game_logic

    def resize(self, w: int, h: int):
        super().resize(w, h)
        self.ui.resize(w, h)

    def render(self, scene, camera):
        # 1️⃣ обычный forward‑render
        super().render(scene, camera)

        # 2️⃣ UI‑overlay (выключаем depth‑test)
        GL.glDisable(GL.GL_DEPTH_TEST)

        # ---- health‑bar (красный) ----
        bar_w = 200
        bar_h = 20
        margin = 20
        hp_ratio = max(0.0, self.game_logic.lives / self.game_logic.MAX_LIVES)
        hp_px = int(bar_w * hp_ratio)

        # фон
        self.ui.draw_rect(margin,
                         self.window.height - bar_h - margin,
                         bar_w, bar_h,
                         ak.Vec3(0.15, 0.15, 0.15))
        # текущий уровень
        self.ui.draw_rect(margin,
                         self.window.height - bar_h - margin,
                         hp_px, bar_h,
                         ak.Vec3(0.9, 0.2, 0.2))

        GL.glEnable(GL.GL_DEPTH_TEST)


# ----------------------------------------------------------------------
# 6️⃣  GameLogic (спаун, столкновения, UI‑данные)
# ----------------------------------------------------------------------
class GameLogic(ak.Node):
    """
    Управляет:
      • спавном астероидов,
      • проверкой столкновений (пуля‑астероид, корабль‑астероид),
      • подсчётом очков и жизней,
      • перезапуском уровня (R).
    """
    MAX_LIVES = 5
    SPAWN_INTERVAL = 1.5   # сек

    def __init__(self, ship: Ship, scene: ak.Scene, window: ak.Window):
        super().__init__("GameLogic")
        self.ship = ship
        self.scene = scene
        self.window = window               # ← теперь GameLogic знает о window
        self.lives = self.MAX_LIVES
        self.score = 0
        self.spawn_timer = 0.0
        self.asteroids: List[Asteroid] = []
        self.bullets: List[Bullet] = []

        # звук взрыва (опциональный)
        self.explosion_chan = None
        if _SOUND_AVAILABLE:
            try:
                self.explosion_snd = mixer.Sound(buffer=b'\x00'*1024)
                self.explosion_chan = self.explosion_snd.play(loops=0)
            except Exception:
                self.explosion_chan = None

    # ------------------------------------------------------------------
    def _spawn_asteroid(self):
        ast = Asteroid(self.ship)
        self.scene.add_child(ast)
        self.asteroids.append(ast)

    # ------------------------------------------------------------------
    def _handle_bullet_asteroid(self, bullet: Bullet, asteroid: Asteroid):
        dist = (bullet.position - asteroid.position).length()
        if dist < asteroid.radius + 0.2:          # 0.2 – радиус пули (приблизительно)
            self.scene.remove_child(bullet)
            self.scene.remove_child(asteroid)
            if bullet in self.bullets:
                self.bullets.remove(bullet)
            if asteroid in self.asteroids:
                self.asteroids.remove(asteroid)
            self.score += 1
            if self.explosion_chan:
                self.explosion_chan.set_volume(0.6)

    # ------------------------------------------------------------------
    def _handle_ship_asteroid(self, asteroid: Asteroid):
        dist = (self.ship.position - asteroid.position).length()
        if dist < asteroid.radius + 0.7:          # корабль ≈ 0.7 м радиус
            self.scene.remove_child(asteroid)
            self.asteroids.remove(asteroid)
            self.lives -= 1
            if self.explosion_chan:
                self.explosion_chan.set_volume(0.8)

    # ------------------------------------------------------------------
    def _reset_level(self):
        """Сбрасывает всё: позиции, жизни, очки, удаляет объекты."""
        self.lives = self.MAX_LIVES
        self.score = 0
        self.spawn_timer = 0.0
        self.ship.position = ak.Vec3(0.0, 0.5, 0.0)
        self.ship.rotation = ak.Vec3(0.0, 0.0, 0.0)

        # удалить астероиды
        for ast in list(self.asteroids):
            self.scene.remove_child(ast)
        self.asteroids.clear()

        # удалить пули
        for bul in list(self.bullets):
            self.scene.remove_child(bul)
        self.bullets.clear()

    # ------------------------------------------------------------------
    def on_update(self, dt: float):
        # --------- спавн астероидов ----------
        self.spawn_timer += dt
        if self.spawn_timer >= self.SPAWN_INTERVAL:
            self._spawn_asteroid()
            self.spawn_timer = 0.0

        # --------- собрать ссылки на пули ----------
        for child in self.scene.traverse():
            if isinstance(child, Bullet) and child not in self.bullets:
                self.bullets.append(child)

        # --------- удалить старые пули ----------
        for bullet in list(self.bullets):
            bullet.age += dt
            if bullet.age >= bullet.LIFETIME:
                self.scene.remove_child(bullet)
                self.bullets.remove(bullet)

        # --------- столкновения пуля‑астероид ----------
        for bullet in list(self.bullets):
            for asteroid in list(self.asteroids):
                self._handle_bullet_asteroid(bullet, asteroid)

        # --------- столкновения корабль‑астероид ----------
        for asteroid in list(self.asteroids):
            self._handle_ship_asteroid(asteroid)

        # --------- перезапуск по клавише R ----------
        if self.window.input.is_key_pressed(glfw.KEY_R):
            self._reset_level()

        # --------- обновление заголовка окна ----------
        self.window.title = f"Space‑Shooter – Lives {self.lives}/{self.MAX_LIVES} | Score {self.score}"


# ----------------------------------------------------------------------
# 7️⃣  Main – сборка сцены и запуск
# ----------------------------------------------------------------------
def main() -> None:
    # -------------------------------------------------
    # 7.1.txt Engine (forward‑pipeline → UI‑renderer)
    # -------------------------------------------------
    engine = ak.Engine(
        width=1280,
        height=720,
        title="Space‑Shooter – Lives 5/5 | Score 0",
        renderer="forward",                # создаём обычный forward‑renderer,
    )

    # -------------------------------------------------
    # 7.2 Сцена: простая точечная освещенность
    # -------------------------------------------------
    sun = ak.DirectionalLight(
        direction=ak.Vec3(-0.5, -1.0, -0.5),
        color=ak.Vec3(1.0, 1.0, 1.0),
        intensity=1.2,
        name="Sun")
    engine.scene.add_child(sun)

    # -------------------------------------------------
    # 7.3 Игрок‑корабль
    # -------------------------------------------------
    ship = Ship()
    ship.bind_input(engine.window.input)          # привязываем InputManager
    ship.scene_root = engine.scene                 # понадобится для спавна пуль
    engine.scene.add_child(ship)

    # -------------------------------------------------
    # 7.4 GameLogic (спавн/столкновения/UI‑данные)
    # -------------------------------------------------
    logic = GameLogic(ship, engine.scene, engine.window)
    engine.scene.add_child(logic)

    # -------------------------------------------------
    # 7.5 UI‑renderer (полоска жизней)
    # -------------------------------------------------
    engine.renderer = UIForwardRenderer(engine.window, ship, logic)

    # -------------------------------------------------
    # 7.6 Камера – следует за кораблём (first‑person)
    # -------------------------------------------------
    original_fly = engine.camera.update_fly

    def ship_camera_fly(self, dt, input_mgr):
        """Камера фиксируется позади и чуть выше корабля."""
        offset = ak.Vec3(0.0, 2.0, 6.0)   # немного назад и вверх
        yaw = math.radians(ship.rotation.y)
        rot_offset = ak.Vec3(
            offset.x * math.cos(yaw) - offset.z * math.sin(yaw),
            offset.y,
            offset.x * math.sin(yaw) + offset.z * math.cos(yaw)
        )
        self.position = ship.position + rot_offset
        self.rotation.y = ship.rotation.y
        self.rotation.x = -15.0

    engine.camera.update_fly = ship_camera_fly.__get__(engine.camera, ak.Camera)

    # -------------------------------------------------
    # 7.7 Запуск игрового цикла
    # -------------------------------------------------
    ak.logger.info("🚀  Space‑Shooter demo started")
    engine.run()
    ak.logger.info("🛑  Demo finished")


# ----------------------------------------------------------------------
# Точка входа
# ----------------------------------------------------------------------
if __name__ == "__main__":
    main()