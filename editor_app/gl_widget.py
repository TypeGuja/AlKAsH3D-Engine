# editor_app/gl_widget.py
"""
OpenGL‑виджет с поддержкой навигации (Orbit / Pan / Zoom), picking,
добавления объектов правой кнопкой, Edit‑Mode (выбор вершины) и т.д.
"""

from __future__ import annotations

import math
import numpy as np
from enum import Enum, auto

# ───── Qt ───────────────────────────────────────────────────────
from PySide6.QtOpenGLWidgets import QOpenGLWidget
from PySide6.QtGui import (
    QColor, QMouseEvent, QWheelEvent, QKeyEvent,
    QIcon, QPalette, QKeySequence, QAction, QActionGroup, QAction
)
from PySide6.QtWidgets import QMenu
from PySide6.QtCore import Qt, QPoint, QSize, QTimer, Signal

# ───── OpenGL (legacy‑pipeline) ──────────────────────────────────────
from OpenGL.GL import *
from OpenGL.GLU import gluPerspective, gluLookAt, gluUnProject, gluProject

# ───── AlKAsH3D ---------------------------------------------------------
from alkash3d.renderer import ForwardRenderer
from alkash3d.scene import Node
from alkash3d.scene.mesh import Mesh
from alkash3d.math.vec3 import Vec3


class DummyWindow:
    """Минимальная «заглушка», требуемая ForwardRenderer."""
    def __init__(self, w: int = 800, h: int = 600):
        self.width = w
        self.height = h


class TransformMode(Enum):
    """Режимы gizmo."""
    TRANSLATE = auto()
    ROTATE    = auto()
    SCALE     = auto()


class GLWidget(QOpenGLWidget):
    """
    OpenGL‑контекст, в котором каждый кадр вызывается
    ForwardRenderer.render(scene, camera).

    Реализованы:
    • навигация камеры (Orbit / Pan / Zoom) – как в Blender,
    • grid / gizmo,
    • wireframe‑режим,
    • picking объектов,
    • picking вершины в Edit‑Mode,
    • добавление куба/сферы/плоскости правой кнопкой мыши (на плоскости Y=0).
    """
    # Сигналы
    object_selected = Signal(object)               # выбранный Mesh
    add_mesh_requested = Signal(object)           # запрос добавить новый Mesh

    # --------------------------------------------------------------
    def __init__(self, scene, camera, parent=None):
        super().__init__(parent)
        self.scene = scene
        self.camera = camera

        # Выбор/трансформация
        self._picked_object: Mesh | None = None
        self._selected_vertex: int | None = None          # индекс вершины в Edit‑Mode
        self._grid_visible = True
        self._gizmo_visible = True
        self._wireframe = False
        self.transform_mode = TransformMode.TRANSLATE

        # Dummy‑окно → ForwardRenderer
        self._dummy = DummyWindow(self.width(), self.height())
        self._renderer: ForwardRenderer | None = None

        # Параметры рендера
        self._background_color = (0.2, 0.2, 0.3, 1.0)
        self._grid_size = 100
        self._grid_step = 1.0

        # ----- навигация камеры -----
        self._cam_target = Vec3(0, 0, 0)          # точка, в которую смотрит камера
        self._cam_distance = 10.0                # расстояние от камеры до target
        self._cam_yaw = 0.0                      # в радианах
        self._cam_pitch = 0.0                    # в радианах
        self._mouse_last: QPoint | None = None
        self._mouse_action: str | None = None    # 'orbit' | 'pan'

        # ----- picking -----
        self._node_id_map: dict[object, int] = {}
        self._id_node_map: dict[int, object] = {}
        self._next_id = 1

        # ----- Edit‑Mode -----
        self._edit_mode = False

        # ----- Drag‑state (новое, избавляемся от AttributeError) -----
        self._dragging: bool = False                 # True, пока удерживается ЛКМ
        self._drag_start: QPoint | None = None       # позиция курсора в начале drag‑а
        self._drag_origin: np.ndarray | None = None # исходная позиция объекта (для перемещения)

    # --------------------------------------------------------------
    #   OpenGL‑инициализация
    # --------------------------------------------------------------
    def initializeGL(self):
        glEnable(GL_DEPTH_TEST)
        glEnable(GL_CULL_FACE)
        self._renderer = ForwardRenderer(self._dummy)

        # Инициализируем параметры камеры из начального положения
        self._cam_distance = (self.camera.position - self._cam_target).length()
        vec = self.camera.position - self._cam_target
        self._cam_yaw = math.atan2(vec.x, vec.z)
        horiz_len = math.hypot(vec.x, vec.z)
        self._cam_pitch = math.atan2(vec.y, horiz_len)

    def resizeGL(self, w: int, h: int):
        self._dummy.width = w
        self._dummy.height = h
        glViewport(0, 0, w, h)

    # --------------------------------------------------------------
    #   Основная отрисовка кадра
    # --------------------------------------------------------------
    def paintGL(self):
        if not self._renderer:
            return

        # ---- Обновляем позицию камеры от навигации -----------------
        self._update_camera_from_target()

        # ---- Очистка буфера, фон ---------------------------------
        r, g, b, a = self._background_color
        glClearColor(r, g, b, a)
        glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT)

        # ---- Режим каркаса ----------------------------------------
        glPolygonMode(GL_FRONT_AND_BACK,
                      GL_LINE if self._wireframe else GL_FILL)

        # ---- Основной рендер ---------------------------------------
        self._renderer.render(self.scene, self.camera)

        # ---- Оверлей (grid, gizmo) ---------------------------------
        glPolygonMode(GL_FRONT_AND_BACK, GL_FILL)
        if self._grid_visible:
            self._render_grid()
        if self._gizmo_visible and self._picked_object:
            self._render_gizmo()

    # --------------------------------------------------------------
    #   Оверлейные элементы
    # --------------------------------------------------------------
    def _render_grid(self):
        glPushAttrib(GL_ENABLE_BIT)
        glDisable(GL_LIGHTING)
        glDisable(GL_TEXTURE_2D)
        glColor3f(0.4, 0.4, 0.4)

        step = self._grid_step
        size = self._grid_size

        glBegin(GL_LINES)
        for i in np.arange(-size, size + step, step):
            glVertex3f(i, 0, -size)
            glVertex3f(i, 0, size)
            glVertex3f(-size, 0, i)
            glVertex3f(size, 0, i)
        glEnd()
        glPopAttrib()

    def _render_gizmo(self):
        """Отрисовка gizmo (или вершины в Edit‑Mode)."""
        node = self._picked_object
        if node is None:
            return

        # Позиция gizmo: объект или выбранная вершина
        pos = node.position
        if self._selected_vertex is not None:
            v = node.vertices[self._selected_vertex]
            pos = Vec3(pos.x + v[0], pos.y + v[1], pos.z + v[2])

        glPushMatrix()
        glTranslatef(pos.x, pos.y, pos.z)

        glLineWidth(2.0)
        axis_len = 1.0

        if self.transform_mode == TransformMode.TRANSLATE:
            # X – красный
            glColor3f(1.0, 0.0, 0.0)
            glBegin(GL_LINES)
            glVertex3f(0, 0, 0)
            glVertex3f(axis_len, 0, 0)
            glEnd()
            # Y – зелёный
            glColor3f(0.0, 1.0, 0.0)
            glBegin(GL_LINES)
            glVertex3f(0, 0, 0)
            glVertex3f(0, axis_len, 0)
            glEnd()
            # Z – синий
            glColor3f(0.0, 0.0, 1.0)
            glBegin(GL_LINES)
            glVertex3f(0, 0, 0)
            glVertex3f(0, 0, axis_len)
            glEnd()
        # (ROTATE и SCALE будут реализованы позже)

        glLineWidth(1.0)
        glPopMatrix()

    # --------------------------------------------------------------
    #   Публичные методы – управление из UI
    # --------------------------------------------------------------
    def toggle_grid(self, visible: bool):
        self._grid_visible = visible
        self.update()

    def toggle_gizmo(self, visible: bool):
        self._gizmo_visible = visible
        self.update()

    def toggle_wireframe(self, enabled: bool):
        self._wireframe = enabled
        self.update()

    def set_background_color(self,
                             r: float, g: float, b: float,
                             a: float = 1.0):
        self._background_color = (r, g, b, a)
        self.update()

    def set_picked_object(self, node: Mesh | None):
        """Выбор объекта (обычный picking)."""
        self._picked_object = node
        self._selected_vertex = None
        self.update()

    def set_transform_mode(self, mode: TransformMode):
        self.transform_mode = mode

    def set_edit_mode(self, enabled: bool):
        self._edit_mode = enabled
        self._selected_vertex = None
        self.update()

    # --------------------------------------------------------------
    #   Навигация камеры (Orbit / Pan / Zoom) – как в Blender
    # --------------------------------------------------------------
    def _update_camera_from_target(self):
        """Обновление позиции/ориентии камеры из target‑а и spherical‑координат."""
        x = self._cam_target.x + self._cam_distance * math.cos(self._cam_pitch) * math.sin(self._cam_yaw)
        y = self._cam_target.y + self._cam_distance * math.sin(self._cam_pitch)
        z = self._cam_target.z + self._cam_distance * math.cos(self._cam_pitch) * math.cos(self._cam_yaw)

        self.camera.position = Vec3(x, y, z)

        # Обновляем yaw/pitch в камере (forward‑vector)
        dir_vec = self._cam_target - self.camera.position
        length = math.sqrt(dir_vec.x ** 2 + dir_vec.y ** 2 + dir_vec.z ** 2)
        if length > 1e-6:
            pitch = math.degrees(math.asin(dir_vec.y / length))
            yaw = math.degrees(math.atan2(dir_vec.x, dir_vec.z))
            self.camera.rotation = Vec3(pitch, yaw, 0.0)

    # --------------------------------------------------------------
    #   Picking (объект) и picking вершины (Edit‑Mode)
    # --------------------------------------------------------------
    def _assign_ids(self):
        """Присваиваем всем узлам уникальные int‑ид."""
        self._node_id_map.clear()
        self._id_node_map.clear()
        self._next_id = 1

        def recurse(node: Node):
            self._node_id_map[node] = self._next_id
            self._id_node_map[self._next_id] = node
            self._next_id += 1
            for ch in node.children:
                recurse(ch)

        recurse(self.scene)

    def _draw_mesh_for_picking(self, mesh: Mesh,
                               color: tuple[int, int, int]):
        glPushMatrix()
        glTranslatef(mesh.position.x, mesh.position.y, mesh.position.z)
        glRotatef(mesh.rotation.x, 1, 0, 0)
        glRotatef(mesh.rotation.y, 0, 1, 0)
        glRotatef(mesh.rotation.z, 0, 0, 1)
        glScalef(mesh.scale.x, mesh.scale.y, mesh.scale.z)

        r, g, b = [c / 255.0 for c in color]
        glColor3f(r, g, b)

        if hasattr(mesh, "indices") and hasattr(mesh, "vertices"):
            glBegin(GL_TRIANGLES)
            for idx in mesh.indices:
                v = mesh.vertices[idx]
                glVertex3f(v[0], v[1], v[2])
            glEnd()
        glPopMatrix()

    def _draw_scene_for_picking(self, node: Node):
        if isinstance(node, Mesh):
            nid = self._node_id_map.get(node, 0)
            r = (nid >> 16) & 0xFF
            g = (nid >> 8) & 0xFF
            b = nid & 0xFF
            self._draw_mesh_for_picking(node, (r, g, b))

        for child in node.children:
            self._draw_scene_for_picking(child)

    def _pick_object(self, mouse_x: int, mouse_y: int) -> Mesh | None:
        """Выбор Mesh под курсором."""
        self._assign_ids()
        w, h = self.width(), self.height()

        # view‑матрица (как в текущем рендере)
        glMatrixMode(GL_PROJECTION)
        glPushMatrix()
        glLoadIdentity()
        gluPerspective(self.camera.fov, w / max(h, 1),
                      self.camera.near, self.camera.far)

        glMatrixMode(GL_MODELVIEW)
        glPushMatrix()
        glLoadIdentity()
        pos = self.camera.position
        gluLookAt(pos.x, pos.y, pos.z,
                  self._cam_target.x, self._cam_target.y, self._cam_target.z,
                  0, 1, 0)

        # рендерим только для picking
        glClearColor(0, 0, 0, 1)
        glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT)
        glDisable(GL_LIGHTING)
        glDisable(GL_TEXTURE_2D)

        self._draw_scene_for_picking(self.scene)

        glFlush()

        pixel = glReadPixels(mouse_x, h - mouse_y - 1, 1, 1,
                             GL_RGB, GL_UNSIGNED_BYTE)
        pixel = np.frombuffer(pixel, dtype=np.uint8)
        r, g, b = int(pixel[0]), int(pixel[1]), int(pixel[2])

        # восстанавливаем матрицы
        glMatrixMode(GL_MODELVIEW)
        glPopMatrix()
        glMatrixMode(GL_PROJECTION)
        glPopMatrix()
        glMatrixMode(GL_MODELVIEW)

        if (r, g, b) == (0, 0, 0):
            return None
        obj_id = (r << 16) + (g << 8) + b
        return self._id_node_map.get(obj_id)

    def _pick_vertex(self, mesh: Mesh, mouse_x: int, mouse_y: int) -> int | None:
        """Выбирает вершину, ближайшую к курсору (порог 10 px)."""
        model = glGetDoublev(GL_MODELVIEW_MATRIX)
        proj = glGetDoublev(GL_PROJECTION_MATRIX)
        viewport = glGetIntegerv(GL_VIEWPORT)

        best_idx = None
        best_dist = 10.0

        for i, v in enumerate(mesh.vertices):
            win = gluProject(v[0], v[1], v[2],
                             model, proj, viewport)
            dx = win[0] - mouse_x
            dy = (viewport[3] - win[1]) - mouse_y
            dist = math.hypot(dx, dy)
            if dist < best_dist:
                best_dist = dist
                best_idx = i
        return best_idx

    # --------------------------------------------------------------
    #   Добавление объекта правой кнопкой (на плоскости Y=0)
    # --------------------------------------------------------------
    def _ray_plane_intersection(self, mx: int, my: int) -> tuple[float, float, float] | None:
        """Ray‑cast от камеры к плоскости Y=0, возвращает (x,y,z) или None."""
        viewport = glGetIntegerv(GL_VIEWPORT)
        model = glGetDoublev(GL_MODELVIEW_MATRIX)
        proj = glGetDoublev(GL_PROJECTION_MATRIX)

        win_x = mx
        win_y = viewport[3] - my

        near = gluUnProject(win_x, win_y, 0.0, model, proj, viewport)
        far = gluUnProject(win_x, win_y, 1.0, model, proj, viewport)

        near = np.array(near, dtype=np.float32)
        far = np.array(far, dtype=np.float32)
        dir_vec = far - near
        if abs(dir_vec[1]) < 1e-6:
            return None
        t = -near[1] / dir_vec[1]          # Y = 0
        if t < 0:
            return None
        point = near + dir_vec * t
        return tuple(point.tolist())

    # --------------------------------------------------------------
    #   Обработчики мыши
    # --------------------------------------------------------------
    def mousePressEvent(self, event: QMouseEvent):
        if event.button() == Qt.LeftButton:
            # ---- Edit‑Mode – выбирать вершину ----
            if self._edit_mode:
                node = self._pick_object(event.x(), event.y())
                self.set_picked_object(node)
                if isinstance(node, Mesh):
                    v_idx = self._pick_vertex(node, event.x(), event.y())
                    if v_idx is not None:
                        self._selected_vertex = v_idx
                        self.object_selected.emit(node)
                        # drag‑вершина
                        self._dragging = True
                        self._drag_start = event.pos()
                        return
                self._selected_vertex = None

            # ---- Обычное выделение объекта ----
            node = self._pick_object(event.x(), event.y())
            self.set_picked_object(node)
            self.object_selected.emit(node)

            if node and self._gizmo_visible:
                # drag‑объект (gizmo)
                self._dragging = True
                self._drag_start = event.pos()
                self._drag_origin = np.array([node.position.x,
                                              node.position.y,
                                              node.position.z],
                                             dtype=np.float32)
            else:
                # ---- Нет объекта → запускаем орбиту ----
                self._mouse_action = "orbit"
                self._mouse_last = event.pos()

        elif event.button() == Qt.MiddleButton:
            # Панорамирование
            self._mouse_action = "pan"
            self._mouse_last = event.pos()

        elif event.button() == Qt.RightButton:
            # Показать контекстное меню добавления объектов
            if not self._edit_mode:
                hit = self._ray_plane_intersection(event.x(), event.y())
                if hit:
                    self._show_add_context_menu(event.globalPos(), hit)

        super().mousePressEvent(event)

    def mouseMoveEvent(self, event: QMouseEvent):
        # ---------- ПАНИРОВАНИЕ ----------
        if self._mouse_action == "pan" and self._mouse_last:
            dx = event.x() - self._mouse_last.x()
            dy = event.y() - self._mouse_last.y()
            factor = 0.005 * self._cam_distance

            right = self._camera_right()
            up = self._camera_up()
            offset = -right * dx * factor + up * dy * factor
            self._cam_target += Vec3(offset[0], offset[1], offset[2])

            self._mouse_last = event.pos()
            self.update()
            return

        # ---------- ОРБИТА ----------
        if self._mouse_action == "orbit" and self._mouse_last:
            dx = event.x() - self._mouse_last.x()
            dy = event.y() - self._mouse_last.y()
            speed = 0.003                     # чувствительность – подбирайте под себя
            self._cam_yaw   -= dx * speed
            self._cam_pitch -= dy * speed

            # Ограничиваем pitch, чтобы камера не «перевернулась» через полюс
            max_pitch = math.radians(89.0)
            self._cam_pitch = max(-max_pitch, min(max_pitch, self._cam_pitch))

            self._mouse_last = event.pos()
            self.update()
            return

        # ---------- Drag (объект / вершина) ----------
        if self._dragging:
            if self._selected_vertex is not None and self._picked_object:
                # перемещение вершины
                dx = event.x() - self._drag_start.x()
                dy = event.y() - self._drag_start.y()
                factor = 0.01
                right = self._camera_right()
                up = self._camera_up()
                offset = right * dx * factor + up * -dy * factor
                v = self._picked_object.vertices[self._selected_vertex]
                self._picked_object.vertices[self._selected_vertex] = v + offset
                self.update()
            elif self._picked_object:
                # перемещение всего объекта
                dx = event.x() - self._drag_start.x()
                dy = event.y() - self._drag_start.y()
                factor = 0.01
                right = self._camera_right()
                up = self._camera_up()
                offset = right * dx * factor + up * -dy * factor
                new_pos = self._drag_origin + offset
                self._picked_object.position = Vec3(float(new_pos[0]),
                                                  float(new_pos[1]),
                                                  float(new_pos[2]))
                self.update()
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event: QMouseEvent):
        if event.button() == Qt.LeftButton and self._dragging:
            # завершение любой drag‑операции (объект или вершина)
            self._dragging = False
            self._drag_start = None
            self._selected_vertex = None

        # окончание орбиты (если была активна)
        if event.button() == Qt.LeftButton and self._mouse_action == "orbit":
            self._mouse_action = None
            self._mouse_last = None

        if event.button() == Qt.MiddleButton and self._mouse_action == "pan":
            self._mouse_action = None
            self._mouse_last = None
        super().mouseReleaseEvent(event)

    def wheelEvent(self, event: QWheelEvent):
        """Zoom – меняем расстояние камеры от target."""
        delta = event.angleDelta().y() / 120.0
        self._cam_distance *= 0.9 ** delta
        self._cam_distance = max(0.1, self._cam_distance)
        self.update()
        super().wheelEvent(event)

    # --------------------------------------------------------------
    #   Вспомогательные методы для контекстного меню
    # --------------------------------------------------------------
    def _show_add_context_menu(self, global_pos: QPoint, hit: tuple[float, float, float]):
        """Отображает меню с вариантами добавления примитивов."""
        menu = QMenu(self)

        act_cube = QAction("Add Cube", menu)
        act_cube.triggered.connect(lambda: self._add_primitive("Cube", hit))
        menu.addAction(act_cube)

        act_sphere = QAction("Add Sphere (placeholder)", menu)
        act_sphere.triggered.connect(lambda: self._add_primitive("Sphere", hit))
        menu.addAction(act_sphere)

        act_plane = QAction("Add Plane", menu)
        act_plane.triggered.connect(lambda: self._add_primitive("Plane", hit))
        menu.addAction(act_plane)

        menu.exec(global_pos)

    def _add_primitive(self, primitive: str, hit: tuple[float, float, float]):
        """Создаёт выбранный примитив в точке ``hit`` и посылает запрос на добавление."""
        if primitive == "Cube":
            verts = np.array([
                [-0.5, -0.5, -0.5], [0.5, -0.5, -0.5],
                [0.5,  0.5, -0.5], [-0.5,  0.5, -0.5],
                [-0.5, -0.5,  0.5], [0.5, -0.5,  0.5],
                [0.5,  0.5,  0.5], [-0.5,  0.5,  0.5]
            ], dtype=np.float32)

            indices = np.array([
                0,1,2, 2,3,0,
                4,5,6, 6,7,4,
                0,4,7, 7,3,0,
                1,5,6, 6,2,1,
                3,2,6, 6,7,3,
                0,1,5, 5,4,0
            ], dtype=np.uint32)

            mesh = Mesh(verts, indices=indices, name="Cube")
        elif primitive == "Sphere":
            mesh = Mesh(name="Sphere")      # placeholder – реального генератора пока нет
        elif primitive == "Plane":
            verts = np.array([
                [-5, 0, -5], [5, 0, -5],
                [5, 0, 5],  [-5, 0, 5]
            ], dtype=np.float32)

            indices = np.array([0,1,2, 2,3,0], dtype=np.uint32)
            mesh = Mesh(verts, indices=indices, name="Plane")
        else:
            return

        mesh.position = Vec3(*hit)
        self.add_mesh_requested.emit(mesh)

    # --------------------------------------------------------------
    #   Векторы камеры (forward / right / up)
    # --------------------------------------------------------------
    def _camera_forward(self) -> np.ndarray:
        dir_vec = self._cam_target - self.camera.position
        v = np.array([dir_vec.x, dir_vec.y, dir_vec.z], dtype=np.float32)
        return v / np.linalg.norm(v)

    def _camera_right(self) -> np.ndarray:
        forward = self._camera_forward()
        world_up = np.array([0, 1, 0], dtype=np.float32)
        right = np.cross(forward, world_up)
        return right / np.linalg.norm(right)

    def _camera_up(self) -> np.ndarray:
        right = self._camera_right()
        forward = self._camera_forward()
        up = np.cross(right, forward)
        return up / np.linalg.norm(up)