#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
Walk‑through demo with proper colors, shadows, collisions
and a few spheres (inline shaders – no external files).
"""

# --------------------------------------------------------------
# 0️⃣ Imports
# --------------------------------------------------------------
import alkash3d as ak                     # public API
import numpy as np
import glfw
import types
from OpenGL import GL

# BaseRenderer lives in the renderer sub‑package
from alkash3d.renderer import BaseRenderer   # ← правильный импорт

# --------------------------------------------------------------
# 1️⃣  Orthographic matrix (для shadow‑map)
# --------------------------------------------------------------
def ortho(left, right, bottom, top, near, far) -> ak.Mat4:
    """Аналог glOrtho, возвращает Mat4 (column‑major)."""
    m = np.identity(4, dtype=np.float32)
    m[0, 0] = 2.0 / (right - left)
    m[1, 1] = 2.0 / (top - bottom)
    m[2, 2] = -2.0 / (far - near)
    m[0, 3] = -(right + left) / (right - left)
    m[1, 3] = -(top + bottom) / (top - bottom)
    m[2, 3] = -(far + near) / (far - near)
    return ak.Mat4(m)


# --------------------------------------------------------------
# 2️⃣  Axis‑Aligned Bounding Box (простая коллизия)
# --------------------------------------------------------------
class AABB:
    def __init__(self, min_vec: ak.Vec3, max_vec: ak.Vec3):
        self.min = min_vec
        self.max = max_vec

    def contains_point(self, p: ak.Vec3) -> bool:
        return (self.min.x <= p.x <= self.max.x and
                self.min.y <= p.y <= self.max.y and
                self.min.z <= p.z <= self.max.z)


# --------------------------------------------------------------
# 3️⃣  Мини‑шэйдер‑класс (компилирует из строк)
# --------------------------------------------------------------
class InlineShader:
    """Wraps a vertex+fragment shader built from string sources."""
    def __init__(self, vertex_src: str, fragment_src: str):
        self._vertex_src   = vertex_src
        self._fragment_src = fragment_src
        self.program = None
        self.compile()

    # ------------------------------------------------------------------
    def compile(self):
        try:
            # vertex
            vert = GL.glCreateShader(GL.GL_VERTEX_SHADER)
            GL.glShaderSource(vert, self._vertex_src)
            GL.glCompileShader(vert)
            self._check_compile(vert, "VERTEX")

            # fragment
            frag = GL.glCreateShader(GL.GL_FRAGMENT_SHADER)
            GL.glShaderSource(frag, self._fragment_src)
            GL.glCompileShader(frag)
            self._check_compile(frag, "FRAGMENT")

            # link
            prog = GL.glCreateProgram()
            GL.glAttachShader(prog, vert)
            GL.glAttachShader(prog, frag)
            GL.glLinkProgram(prog)
            self._check_link(prog)

            # cleanup
            GL.glDeleteShader(vert)
            GL.glDeleteShader(frag)

            self.program = prog
        except Exception as e:
            raise RuntimeError(f"[InlineShader] compilation failed: {e}")

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

    # ------------------------------------------------------------------
    def set_uniform_mat4(self, name: str, mat):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        # `mat` может быть Mat4 (имеет .to_gl()) или уже готовый ndarray
        if hasattr(mat, "to_gl"):
            data = mat.to_gl()
        else:
            data = np.array(mat, dtype=np.float32)
        GL.glUniformMatrix4fv(loc, 1, GL.GL_FALSE, data)

    def set_uniform_vec3(self, name: str, vec):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        if hasattr(vec, "as_np"):
            data = vec.as_np()
        else:
            data = np.array(vec, dtype=np.float32)
        GL.glUniform3fv(loc, 1, data)

    def set_uniform_int(self, name: str, value: int):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        GL.glUniform1i(loc, int(value))

    def set_uniform_float(self, name: str, value: float):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        GL.glUniform1f(loc, float(value))


# --------------------------------------------------------------
# 4️⃣  Шейдер‑коды (внутри строк)
# --------------------------------------------------------------
FORWARD_VERT_SRC = """#version 450 core
layout(location = 0) in vec3 aPos;
layout(location = 1.txt) in vec3 aNormal;
layout(location = 2) in vec2 aTexCoord;

uniform mat4 uModel;
uniform mat4 uView;
uniform mat4 uProj;
uniform mat4 uLightSpace;                // для shadow‑mapping

out vec3 vFragPos;
out vec3 vNormal;
out vec2 vTexCoord;
out vec4 vLightSpacePos;

void main()
{
    mat4 modelView = uView * uModel;
    vFragPos = vec3(uModel * vec4(aPos, 1.txt.0));
    vNormal  = mat3(transpose(inverse(uModel))) * aNormal;
    vTexCoord = aTexCoord;
    vLightSpacePos = uLightSpace * uModel * vec4(aPos, 1.txt.0);
    gl_Position = uProj * modelView * vec4(aPos, 1.txt.0);
}
"""

FORWARD_FRAG_SRC = """#version 450 core
in vec3 vFragPos;
in vec3 vNormal;
in vec2 vTexCoord;
in vec4 vLightSpacePos;

uniform sampler2D uAlbedo;      // (не используется, но объявлен)
uniform sampler2D uShadowMap;    // depth‑map от света

uniform vec3 uLightDir;         // направление света (мировое)
uniform vec3 uLightColor;
uniform float uLightIntensity;
uniform vec3 uCamPos;            // (не используется)
uniform vec3 uColor;            // базовый цвет объекта

out vec4 FragColor;

// -------------------------------------------------------
// Жёсткая тень + небольшой bias, зависящий от угла наклона
// -------------------------------------------------------
float computeShadow()
{
    // из light‑space → [0,1.txt]
    vec3 proj = vLightSpacePos.xyz / vLightSpacePos.w;
    proj = proj * 0.5 + 0.5;

    // если координаты выходят за пределы depth‑map → считаем, что света нет препятствия
    if (proj.z > 1.txt.0 ||
        proj.x < 0.0 || proj.x > 1.txt.0 ||
        proj.y < 0.0 || proj.y > 1.txt.0)
        return 1.txt.0;

    float currentDepth = proj.z;
    float bias = max(0.005 * (1.txt.0 - dot(normalize(vNormal), -uLightDir)), 0.001);
    float closestDepth = texture(uShadowMap, proj.xy).r;
    return currentDepth - bias > closestDepth ? 0.0 : 1.txt.0;
}

void main()
{
    vec3 N = normalize(vNormal);
    vec3 L = normalize(-uLightDir);                 // луч света
    float diff = max(dot(N, L), 0.0);

    // базовый ambient‑цвет (не слишком яркий)
    vec3 ambient = 0.12 * uLightColor;

    float shadow = computeShadow();

    // итоговое освещение
    vec3 lighting = ambient + shadow * diff * uLightColor * uLightIntensity;

    // умножаем на цвет объекта
    FragColor = vec4(lighting * uColor, 1.txt.0);
}
"""

SHADOW_DEPTH_VERT_SRC = """#version 450 core
layout(location = 0) in vec3 aPos;

uniform mat4 uModel;
uniform mat4 uLightSpace;   // Light projection * Light view

void main()
{
    gl_Position = uLightSpace * uModel * vec4(aPos, 1.txt.0);
}
"""

SHADOW_DEPTH_FRAG_SRC = """#version 450 core
void main() { }
"""


# --------------------------------------------------------------
# 5️⃣  ShadowRenderer (forward + depth‑pass)
# --------------------------------------------------------------
class ShadowRenderer(BaseRenderer):
    """Forward‑pipeline + Shadow‑Mapping."""

    def __init__(self, window):
        self.window = window
        self.width, self.height = window.width, window.height

        # ----- compile inline shaders ---------------------------------
        self.main_shader   = InlineShader(FORWARD_VERT_SRC,   FORWARD_FRAG_SRC)
        self.shadow_shader = InlineShader(SHADOW_DEPTH_VERT_SRC, SHADOW_DEPTH_FRAG_SRC)

        # ----- fallback white texture (sampler 0) --------------------
        self._create_default_white_texture()

        # ----- depth‑map (shadow‑texture, sampler 1.txt) -----------------
        self._setup_shadow_map()

        GL.glEnable(GL.GL_DEPTH_TEST)

    # ------------------------------------------------------------------
    def _create_default_white_texture(self):
        """1.txt×1.txt белая текстура → unit 0."""
        self.white_tex = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.white_tex)
        white_px = (255).to_bytes(1, "little") * 4   # RGBA 255,255,255,255
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGBA8,
                        1, 1, 0,
                        GL.GL_RGBA, GL.GL_UNSIGNED_BYTE, white_px)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)

    # ------------------------------------------------------------------
    def _setup_shadow_map(self, res: int = 2048):
        """Создаёт FBO + depth‑текстуру, которая будет shadow‑map‑ой."""
        self.shadow_res = res

        # depth‑texture
        self.shadow_tex = GL.glGenTextures(1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.shadow_tex)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_DEPTH_COMPONENT16,
                        res, res, 0,
                        GL.GL_DEPTH_COMPONENT, GL.GL_FLOAT, None)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_NEAREST)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_WRAP_S, GL.GL_CLAMP_TO_BORDER)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_WRAP_T, GL.GL_CLAMP_TO_BORDER)
        border = np.array([1.0, 1.0, 1.0, 1.0], dtype=np.float32)
        GL.glTexParameterfv(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_BORDER_COLOR, border)

        # FBO
        self.shadow_fbo = GL.glGenFramebuffers(1)
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, self.shadow_fbo)
        GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER, GL.GL_DEPTH_ATTACHMENT,
                                 GL.GL_TEXTURE_2D, self.shadow_tex, 0)
        GL.glDrawBuffer(GL.GL_NONE)
        GL.glReadBuffer(GL.GL_NONE)

        if GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER) != GL.GL_FRAMEBUFFER_COMPLETE:
            raise RuntimeError("Shadow FBO incomplete!")
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, 0)

    # ------------------------------------------------------------------
    def resize(self, w: int, h: int) -> None:
        """Engine вызывает этот метод при изменении окна."""
        self.width, self.height = w, h
        GL.glViewport(0, 0, w, h)

    # ------------------------------------------------------------------
    def render(self, scene, camera):
        # -------------------------------------------------
        # 1️⃣ Depth‑pass от DirectionalLight → shadow‑map
        # -------------------------------------------------
        sun = None
        for node in scene.traverse():
            if isinstance(node, ak.DirectionalLight):
                sun = node
                break
        if sun is None:
            raise RuntimeError("В сцене нет DirectionalLight (нужен для теней)")

        # Ортографическая проекция, покрывающая большую часть уровня
        ortho_sz = 20.0
        light_proj = ortho(-ortho_sz, ortho_sz,
                           -ortho_sz, ortho_sz,
                           1.0, 30.0)            # near / far

        # Позиция солнца (отодвинут вдоль направления света)
        light_pos = -sun.direction.as_np() * 15.0
        light_view = ak.Mat4.look_at(
            light_pos,
            np.array([0.0, 0.0, 0.0], dtype=np.float32),
            np.array([0.0, 1.0, 0.0], dtype=np.float32)
        )
        light_space = (light_proj @ light_view).to_gl()   # numpy‑array

        # ---- рендер в shadow‑FBO ----
        GL.glViewport(0, 0, self.shadow_res, self.shadow_res)
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, self.shadow_fbo)
        GL.glClear(GL.GL_DEPTH_BUFFER_BIT)

        self.shadow_shader.use()
        self.shadow_shader.set_uniform_mat4("uLightSpace", light_space)

        for node in scene.traverse():
            if hasattr(node, "draw"):
                model = node.get_world_matrix().to_gl()
                self.shadow_shader.set_uniform_mat4("uModel", model)
                node.draw()

        # -------------------------------------------------
        # 2️⃣ Forward‑проход (используем shadow‑map)
        # -------------------------------------------------
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER, 0)
        GL.glViewport(0, 0, self.width, self.height)
        GL.glClearColor(0.07, 0.07, 0.12, 1.0)
        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)

        self.main_shader.use()
        view = camera.get_view_matrix()
        proj = camera.get_projection_matrix(self.width / self.height)

        self.main_shader.set_uniform_mat4("uView", view)
        self.main_shader.set_uniform_mat4("uProj", proj)
        self.main_shader.set_uniform_mat4("uLightSpace", light_space)

        # uniform‑ы света
        self.main_shader.set_uniform_vec3("uLightDir", sun.direction.as_np())
        self.main_shader.set_uniform_vec3("uLightColor", sun.color.as_np())
        self.main_shader.set_uniform_float("uLightIntensity", sun.intensity)
        self.main_shader.set_uniform_vec3("uCamPos", camera.position.as_np())

        # bind fallback albedo → unit 0
        GL.glActiveTexture(GL.GL_TEXTURE0)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.white_tex)
        self.main_shader.set_uniform_int("uAlbedo", 0)

        # bind shadow‑map → unit 1.txt
        GL.glActiveTexture(GL.GL_TEXTURE1)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.shadow_tex)
        self.main_shader.set_uniform_int("uShadowMap", 1)

        # --------------------------- рисуем ---------------------------
        for node in scene.traverse():
            if hasattr(node, "draw"):
                model = node.get_world_matrix().to_gl()
                self.main_shader.set_uniform_mat4("uModel", model)

                # Цвет объекта (по‑умолчанию белый)
                if hasattr(node, "color"):
                    self.main_shader.set_uniform_vec3("uColor", node.color.as_np())
                else:
                    self.main_shader.set_uniform_vec3(
                        "uColor", np.array([1.0, 1.0, 1.0], dtype=np.float32))

                node.draw()


# --------------------------------------------------------------
# 6️⃣  Фабрика простого куба (24 вершины, отдельные нормали)
# --------------------------------------------------------------
def make_box_mesh(name: str = "Box") -> ak.Mesh:
    pos = [
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
    ]
    norm = [
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
    ]
    inds = [
        0,1,2, 0,2,3,       # front
        4,5,6, 4,6,7,       # back
        8,9,10, 8,10,11,    # left
        12,13,14, 12,14,15, # right
        16,17,18, 16,18,19, # top
        20,21,22, 20,22,23, # bottom
    ]
    verts = np.array(pos,  dtype=np.float32)
    norms = np.array(norm, dtype=np.float32)
    inds  = np.array(inds, dtype=np.uint32)
    return ak.Mesh(vertices=verts, normals=norms, indices=inds, name=name)


# --------------------------------------------------------------
# 6️⃣  Фабрика сферы (UV‑сфера, простая генерация)
# --------------------------------------------------------------
def make_sphere_mesh(radius: float = 1.0,
                    stacks: int = 16,
                    slices: int = 16,
                    name: str = "Sphere") -> ak.Mesh:
    """
    Генерирует UV‑сферу.
    Позиции – массив (N,3), нормали – массив (N,3), индексы – треугольники.
    """
    positions = []
    normals   = []

    for i in range(stacks + 1):
        lat = np.pi / 2 - i * np.pi / stacks         # от +π/2 до -π/2
        sin_lat = np.sin(lat)
        cos_lat = np.cos(lat)

        for j in range(slices + 1):
            lon = 2 * np.pi * j / slices
            sin_lon = np.sin(lon)
            cos_lon = np.cos(lon)

            x = cos_lat * cos_lon
            y = sin_lat
            z = cos_lat * sin_lon

            positions.append([radius * x, radius * y, radius * z])
            normals.append([x, y, z])                 # для единичной сферы

    positions = np.array(positions, dtype=np.float32)
    normals   = np.array(normals,   dtype=np.float32)

    # индексы
    indices = []
    for i in range(stacks):
        for j in range(slices):
            first  = i * (slices + 1) + j
            second = first + slices + 1

            indices.extend([first, second, first + 1])
            indices.extend([second, second + 1, first + 1])

    indices = np.array(indices, dtype=np.uint32)

    return ak.Mesh(vertices=positions,
                  normals=normals,
                  indices=indices,
                  name=name)


# --------------------------------------------------------------
# 7️⃣  Коллизии + пользовательский update_fly (с учётом AABB)
# --------------------------------------------------------------
def make_collision_update(colliders, speed: float = 5.0):
    """Создаёт функцию, которую будем привязывать к Camera.update_fly."""
    def update(self, dt, input_mgr):
        # ----- перемещение (WASD + Space/Shift) -----
        move = ak.Vec3()
        if input_mgr.is_key_pressed(glfw.KEY_W):
            move += self.forward
        if input_mgr.is_key_pressed(glfw.KEY_S):
            move -= self.forward
        if input_mgr.is_key_pressed(glfw.KEY_A):
            move -= self.right
        if input_mgr.is_key_pressed(glfw.KEY_D):
            move += self.right
        if input_mgr.is_key_pressed(glfw.KEY_SPACE):
            move += self.up
        if input_mgr.is_key_pressed(glfw.KEY_LEFT_SHIFT):
            move -= self.up

        if move.length() > 0.0:
            move = move.normalized() * speed * dt

        # Предлагаемое новое положение (Y фиксируем – высота глаз)
        new_pos = self.position + move
        new_pos = ak.Vec3(new_pos.x, self.position.y, new_pos.z)

        # Проверяем столкновение со всеми AABB
        for box in colliders:
            if box.contains_point(new_pos):
                new_pos = self.position   # откатываем в прежнее положение
                break

        self.position = new_pos

        # ----- мышиный look (fly‑style) -----
        dx, dy = input_mgr.get_mouse_delta()
        self.rotation.y += dx * 0.1
        self.rotation.x += dy * 0.1
        self.rotation.x = max(-89.0, min(89.0, self.rotation.x))
    return update


# --------------------------------------------------------------
# 8️⃣  Сборка сцены и запуск движка
# --------------------------------------------------------------
def main():
    # -------------------------------------------------
    # 8.1.txt Engine (по‑умолчанию forward‑pipeline)
    # -------------------------------------------------
    engine = ak.Engine(
        width=1280,
        height=720,
        title="AlKAsH3D – Walk‑through + Shadows (inline shaders + spheres)",
        renderer="forward",               # временно, подменим ниже
    )

    # -------------------------------------------------
    # 8.2 Подменяем рендерер на наш ShadowRenderer
    # -------------------------------------------------
    engine.renderer = ShadowRenderer(engine.window)

    # -------------------------------------------------
    # 8.3 Пол (большой квадрат)
    # -------------------------------------------------
    plane_vertices = np.array([
        -10.0, 0.0, -10.0,
         10.0, 0.0, -10.0,
         10.0, 0.0,  10.0,
        -10.0, 0.0,  10.0,
    ], dtype=np.float32)
    plane_normals = np.tile([0.0, 1.0, 0.0], 4)          # нормаль вверх
    plane_indices = np.array([0, 1, 2, 0, 2, 3], dtype=np.uint32)

    plane = ak.Mesh(vertices=plane_vertices,
                    normals=plane_normals,
                    indices=plane_indices,
                    name="Ground")
    engine.scene.add_child(plane)

    # -------------------------------------------------
    # 8.4 Добавляем кубы‑препятствия (разные цвета)
    # -------------------------------------------------
    colliders = []          # список AABB‑коллайдеров

    def add_box(pos: ak.Vec3, scale: ak.Vec3, color: ak.Vec3, name: str):
        box = make_box_mesh(name)
        box.position = pos
        box.scale    = scale
        box.color    = color            # пользовательский атрибут
        engine.scene.add_child(box)

        half = scale * 0.5
        min_corner = pos - half
        max_corner = pos + half
        colliders.append(AABB(min_corner, max_corner))

    # три куба разных цветов
    add_box(ak.Vec3( 2.0, 0.0, -2.0), ak.Vec3(1.0, 2.0, 1.0),
            ak.Vec3(1.0, 0.2, 0.2), "BoxRed")
    add_box(ak.Vec3(-3.0, 0.0,  1.5), ak.Vec3(2.0, 3.0, 2.0),
            ak.Vec3(0.2, 1.0, 0.2), "BoxGreen")
    add_box(ak.Vec3( 0.0, 0.0, -5.0), ak.Vec3(1.5, 1.5, 1.5),
            ak.Vec3(0.2, 0.2, 1.0), "BoxBlue")

    # -------------------------------------------------
    # 8.5 **Сферы** – новые объекты (разные цвета и радиусы)
    # -------------------------------------------------
    def add_sphere(pos: ak.Vec3, radius: float, color: ak.Vec3, name: str):
        # геометрия – единичная сфера; масштабируем через .scale
        sphere = make_sphere_mesh(radius=1.0, stacks=16, slices=16, name=name)
        sphere.position = pos
        sphere.scale    = ak.Vec3(radius, radius, radius)   # масштаб ≈ радиус
        sphere.color    = color
        engine.scene.add_child(sphere)

        # AABB‑коллайдер (квадратный, охватывающий сферу)
        half = ak.Vec3(radius, radius, radius)
        min_corner = pos - half
        max_corner = pos + half
        colliders.append(AABB(min_corner, max_corner))

    # три сферы разных цветов/радиусов
    add_sphere(ak.Vec3(-2.0, 0.6,  3.0), 0.6,
               ak.Vec3(1.0, 1.0, 0.2), "SphereYellow")
    add_sphere(ak.Vec3( 4.0, 1.0, -3.5), 1.0,
               ak.Vec3(0.9, 0.2, 0.9), "SphereMagenta")
    add_sphere(ak.Vec3( 0.0, 0.8,  0.0), 0.8,
               ak.Vec3(0.2, 0.8, 0.8), "SphereCyan")

    # -------------------------------------------------
    # 8.6 Солнце – DirectionalLight
    # -------------------------------------------------
    sun = ak.DirectionalLight(
        direction=ak.Vec3(-0.5, -1.0, -0.5),   # свет падает сверху‑справа‑назад
        color=ak.Vec3(1.0, 1.0, 0.95),
        intensity=1.8,
        name="Sun",
    )
    engine.scene.add_child(sun)

    # -------------------------------------------------
    # 8.7 Камера – стартовая позиция (высота глаз ≈ 1.txt.6 м)
    # -------------------------------------------------
    engine.camera.position = ak.Vec3(0.0, 1.6, 5.0)

    # -------------------------------------------------
    # 8.8 Подменяем update_fly у камеры (добавляем AABB‑коллизию)
    # -------------------------------------------------
    engine.camera.update_fly = types.MethodType(
        make_collision_update(colliders, speed=5.0), engine.camera
    )

    # -------------------------------------------------
    # 8.9 Запускаем основной цикл движка
    # -------------------------------------------------
    ak.logger.info("Запуск демо‑игры: walk‑through + shadows + spheres")
    engine.run()
    ak.logger.info("Демо завершено")


# --------------------------------------------------------------
# Точка входа
# --------------------------------------------------------------
if __name__ == "__main__":
    main()