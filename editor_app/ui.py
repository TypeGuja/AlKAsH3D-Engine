# editor_app/ui.py
"""
Qt‑интерфейс редактора AlKAsH3D, стиlizованный под Unity/Blender.

Функции:
 • навигация камеры (Orbit / Pan / Zoom) – как в Blender,
 • правый клик → добавление куба на плоскости Y = 0,
 • Edit‑Mode (Tab) – выбор и перемещение отдельной вершины,
 • стандартные панели: Hierarchy, Inspector, Project, Console,
 • меню, тул‑бар, Undo/Redo, Play‑Pause‑Stop и пр.
"""

import time
from pathlib import Path
import numpy as np

# ───── Qt ───────────────────────────────────────────────────────
from PySide6.QtWidgets import (
    QMainWindow, QTreeWidget, QTreeWidgetItem, QGroupBox,
    QLineEdit, QLabel, QPushButton, QFileDialog,
    QMessageBox, QVBoxLayout, QHBoxLayout, QFormLayout,
    QWidget, QDockWidget, QToolBar, QStatusBar, QMenu,
    QCheckBox, QDoubleSpinBox, QColorDialog,
    QListWidget, QTextEdit, QApplication,
)
from PySide6.QtGui import (
    QColor, QPalette, QKeySequence, QAction, QActionGroup,
)
from PySide6.QtCore import Qt, QTimer, QPoint, QSize, Signal

# ───── AlKAsH3D ───────────────────────────────────────────────────
from alkash3d.scene import Scene, Node
from alkash3d.scene.mesh import Mesh
from alkash3d.scene.camera import Camera
from alkash3d.scene.light import DirectionalLight, PointLight, SpotLight
from alkash3d.math.vec3 import Vec3
from alkash3d.utils.loader import load_obj

# ───── Виджеты проекта ────────────────────────────────────────
from .gl_widget import GLWidget, TransformMode
from .scene_io import save_scene, load_scene, node_to_dict, dict_to_node


# ----------------------------------------------------------------------
#   Тёмная тема (Unity‑подобный стиль)
# ----------------------------------------------------------------------
class EditorTheme:
    @staticmethod
    def apply_dark_theme(app: QApplication):
        from PySide6.QtWidgets import QStyleFactory

        app.setStyle(QStyleFactory.create("Fusion"))
        dark = QPalette()
        dark.setColor(QPalette.Window, QColor(53, 53, 53))
        dark.setColor(QPalette.WindowText, Qt.white)
        dark.setColor(QPalette.Base, QColor(25, 25, 25))
        dark.setColor(QPalette.AlternateBase, QColor(53, 53, 53))
        dark.setColor(QPalette.ToolTipBase, Qt.white)
        dark.setColor(QPalette.ToolTipText, Qt.white)
        dark.setColor(QPalette.Text, Qt.white)
        dark.setColor(QPalette.Button, QColor(53, 53, 53))
        dark.setColor(QPalette.ButtonText, Qt.white)
        dark.setColor(QPalette.BrightText, Qt.red)
        dark.setColor(QPalette.Link, QColor(42, 130, 218))
        dark.setColor(QPalette.Highlight, QColor(42, 130, 218))
        dark.setColor(QPalette.HighlightedText, Qt.black)
        app.setPalette(dark)


# ----------------------------------------------------------------------
#   Инспектор (ComponentEditor)
# ----------------------------------------------------------------------
class ComponentEditor(QGroupBox):
    """Редактор компонентов выбранного узла."""

    def __init__(self, parent=None):
        super().__init__("Components", parent)
        self.layout = QVBoxLayout(self)
        self.current_node: Node | None = None

    # ------------------------------------------------------------------
    def set_node(self, node: Node | None):
        """Устанавливает узел, свойства которого показываются в инспекторе."""
        self.current_node = node
        self._clear_layout()
        if node is None:
            return

        # ---- Имя -------------------------------------------------
        name_l = QHBoxLayout()
        name_l.addWidget(QLabel("Name:"))
        self.name_edit = QLineEdit(node.name)
        self.name_edit.editingFinished.connect(self._on_name_changed)
        name_l.addWidget(self.name_edit)
        self.layout.addLayout(name_l)

        # ---- Трансформация ---------------------------------------
        self._add_transform_component(node)

        # ---- Специфические компоненты ----------------------------
        if isinstance(node, Camera):
            self._add_camera_component(node)
        elif isinstance(node, (DirectionalLight, PointLight, SpotLight)):
            self._add_light_component(node)
        elif isinstance(node, Mesh):
            self._add_mesh_component(node)

    # ------------------------------------------------------------------
    def _clear_layout(self):
        while self.layout.count():
            child = self.layout.takeAt(0)
            if child.widget():
                child.widget().deleteLater()
            elif child.layout():
                while child.layout().count():
                    w = child.layout().takeAt(0).widget()
                    if w:
                        w.deleteLater()

    # ------------------------------------------------------------------
    def _on_name_changed(self):
        if self.current_node:
            n = self.name_edit.text().strip()
            if n:
                self.current_node.name = n

    # ------------------------------------------------------------------
    #   Трансформация (position, rotation, scale)
    # ------------------------------------------------------------------
    def _add_transform_component(self, node: Node):
        grp = QGroupBox("Transform")
        form = QFormLayout(grp)

        # Position
        self.pos_x = QDoubleSpinBox()
        self.pos_x.setRange(-1000, 1000)
        self.pos_x.setValue(float(node.position.x))
        self.pos_x.valueChanged.connect(lambda v: self._on_transform('position', v, 0))

        self.pos_y = QDoubleSpinBox()
        self.pos_y.setRange(-1000, 1000)
        self.pos_y.setValue(float(node.position.y))
        self.pos_y.valueChanged.connect(lambda v: self._on_transform('position', v, 1))

        self.pos_z = QDoubleSpinBox()
        self.pos_z.setRange(-1000, 1000)
        self.pos_z.setValue(float(node.position.z))
        self.pos_z.valueChanged.connect(lambda v: self._on_transform('position', v, 2))

        pos_l = QHBoxLayout()
        for lab, sb in (("X:", self.pos_x), ("Y:", self.pos_y), ("Z:", self.pos_z)):
            pos_l.addWidget(QLabel(lab))
            pos_l.addWidget(sb)
        form.addRow("Position", pos_l)

        # Rotation
        self.rot_x = QDoubleSpinBox()
        self.rot_x.setRange(-360, 360)
        self.rot_x.setValue(float(node.rotation.x))
        self.rot_x.valueChanged.connect(lambda v: self._on_transform('rotation', v, 0))

        self.rot_y = QDoubleSpinBox()
        self.rot_y.setRange(-360, 360)
        self.rot_y.setValue(float(node.rotation.y))
        self.rot_y.valueChanged.connect(lambda v: self._on_transform('rotation', v, 1))

        self.rot_z = QDoubleSpinBox()
        self.rot_z.setRange(-360, 360)
        self.rot_z.setValue(float(node.rotation.z))
        self.rot_z.valueChanged.connect(lambda v: self._on_transform('rotation', v, 2))

        rot_l = QHBoxLayout()
        for lab, sb in (("X:", self.rot_x), ("Y:", self.rot_y), ("Z:", self.rot_z)):
            rot_l.addWidget(QLabel(lab))
            rot_l.addWidget(sb)
        form.addRow("Rotation", rot_l)

        # Scale
        self.scale_x = QDoubleSpinBox()
        self.scale_x.setRange(0.001, 10)
        self.scale_x.setValue(float(node.scale.x))
        self.scale_x.valueChanged.connect(lambda v: self._on_transform('scale', v, 0))

        self.scale_y = QDoubleSpinBox()
        self.scale_y.setRange(0.001, 10)
        self.scale_y.setValue(float(node.scale.y))
        self.scale_y.valueChanged.connect(lambda v: self._on_transform('scale', v, 1))

        self.scale_z = QDoubleSpinBox()
        self.scale_z.setRange(0.001, 10)
        self.scale_z.setValue(float(node.scale.z))
        self.scale_z.valueChanged.connect(lambda v: self._on_transform('scale', v, 2))

        scl_l = QHBoxLayout()
        for lab, sb in (("X:", self.scale_x), ("Y:", self.scale_y), ("Z:", self.scale_z)):
            scl_l.addWidget(QLabel(lab))
            scl_l.addWidget(sb)
        form.addRow("Scale", scl_l)

        self.layout.addWidget(grp)

    def _on_transform(self, prop: str, value: float, axis: int):
        if not self.current_node:
            return
        vec = getattr(self.current_node, prop)
        if axis == 0:
            vec.x = value
        elif axis == 1:
            vec.y = value
        elif axis == 2:
            vec.z = value
        setattr(self.current_node, prop, vec)

    # ------------------------------------------------------------------
    #   Камера
    # ------------------------------------------------------------------
    def _add_camera_component(self, cam: Camera):
        grp = QGroupBox("Camera")
        form = QFormLayout(grp)

        fov = QDoubleSpinBox()
        fov.setRange(1, 179)
        fov.setValue(cam.fov)
        fov.valueChanged.connect(lambda v: setattr(cam, 'fov', v))
        form.addRow("FOV", fov)

        near = QDoubleSpinBox()
        near.setRange(0.01, 1000)
        near.setValue(cam.near)
        near.valueChanged.connect(lambda v: setattr(cam, 'near', v))
        form.addRow("Near", near)

        far = QDoubleSpinBox()
        far.setRange(0.1, 100000)
        far.setValue(cam.far)
        far.valueChanged.connect(lambda v: setattr(cam, 'far', v))
        form.addRow("Far", far)

        bg = QPushButton("Background")
        bg.clicked.connect(lambda: self._pick_camera_background(cam))
        form.addRow("Background", bg)

        self.layout.addWidget(grp)

    def _pick_camera_background(self, cam: Camera):
        col = QColorDialog.getColor(parent=self)
        if col.isValid():
            glw: GLWidget = self.parent().parent().gl_widget
            glw.set_background_color(col.redF(), col.greenF(),
                                      col.blueF(), col.alphaF())

    # ------------------------------------------------------------------
    #   Свет
    # ------------------------------------------------------------------
    def _add_light_component(self, light):
        grp = QGroupBox("Light")
        form = QFormLayout(grp)

        intensity = QDoubleSpinBox()
        intensity.setRange(0, 10)
        intensity.setSingleStep(0.1)
        intensity.setValue(getattr(light, "intensity", 1.0))
        intensity.valueChanged.connect(lambda v: setattr(light, "intensity", v))
        form.addRow("Intensity", intensity)

        col_btn = QPushButton("Color")
        col_btn.clicked.connect(lambda: self._pick_light_color(light))
        form.addRow("Color", col_btn)

        self.layout.addWidget(grp)

    def _pick_light_color(self, light):
        col = QColorDialog.getColor(parent=self)
        if col.isValid() and hasattr(light, "color"):
            light.color = Vec3(col.redF(), col.greenF(), col.blueF())

    # ------------------------------------------------------------------
    #   Mesh
    # ------------------------------------------------------------------
    def _add_mesh_component(self, mesh: Mesh):
        grp = QGroupBox("Mesh Renderer")
        form = QFormLayout(grp)

        wf = QCheckBox("Wireframe")
        wf.toggled.connect(lambda v: self.parent().parent().gl_widget.toggle_wireframe(v))
        form.addRow("", wf)

        if hasattr(mesh, "material"):
            mat_btn = QPushButton("Material colour")
            mat_btn.clicked.connect(lambda: self._pick_mesh_material(mesh))
            form.addRow("Material", mat_btn)

        self.layout.addWidget(grp)

    def _pick_mesh_material(self, mesh: Mesh):
        col = QColorDialog.getColor(parent=self)
        if col.isValid() and hasattr(mesh, "material"):
            mesh.material.color = Vec3(col.redF(), col.greenF(), col.blueF())


# ----------------------------------------------------------------------
#   Dock‑окно иерархии
# ----------------------------------------------------------------------
class HierarchyDock(QDockWidget):
    def __init__(self, parent: QMainWindow):
        super().__init__("Hierarchy", parent)
        self.main = parent

        w = QWidget()
        l = QVBoxLayout(w)

        self.search = QLineEdit()
        self.search.setPlaceholderText("Search…")
        self.search.textChanged.connect(self._filter)
        l.addWidget(self.search)

        self.tree = QTreeWidget()
        self.tree.setHeaderLabel("Scene")
        self.tree.itemClicked.connect(self._on_item_clicked)
        self.tree.itemDoubleClicked.connect(self._on_item_double_clicked)

        # Важно: сигнал контекстного меню ставим на viewport,
        # иначе он не срабатывает.
        self.tree.viewport().setContextMenuPolicy(Qt.CustomContextMenu)
        self.tree.viewport().customContextMenuRequested.connect(self._show_context_menu)

        l.addWidget(self.tree)

        self.setWidget(w)
        self.refresh()

    # ------------------------------------------------------------------
    def refresh(self):
        """Перестроить дерево из текущей сцены."""
        self.tree.clear()

        def recurse(node: Node,
                    parent_item: QTreeWidgetItem | None = None):
            it = QTreeWidgetItem([node.name])
            it.setData(0, Qt.UserRole, node)
            it.setFlags(it.flags() | Qt.ItemIsEditable)

            if parent_item is None:
                self.tree.addTopLevelItem(it)
            else:
                parent_item.addChild(it)

            for ch in node.children:
                recurse(ch, it)

        recurse(self.main.scene)
        self.tree.expandAll()
        # Обновляем информационный счётчик объектов в статус‑баре.
        self.main._update_scene_info()

    # ------------------------------------------------------------------
    def _filter(self, txt: str):
        root = self.tree.invisibleRootItem()
        stack = [root]
        while stack:
            cur = stack.pop()
            for i in range(cur.childCount()):
                child = cur.child(i)
                node: Node = child.data(0, Qt.UserRole)
                child.setHidden(txt.lower() not in node.name.lower())
                stack.append(child)

    # ------------------------------------------------------------------
    def _on_item_clicked(self, item: QTreeWidgetItem, column: int):
        node: Node = item.data(0, Qt.UserRole)
        self.main._select_node(node)

    # ------------------------------------------------------------------
    def _on_item_double_clicked(self, item: QTreeWidgetItem, column: int):
        self.tree.editItem(item, column)

    # ------------------------------------------------------------------
    def _show_context_menu(self, point):
        item = self.tree.itemAt(point)
        menu = QMenu(self)

        if item:
            del_act = QAction("Delete", self)
            del_act.triggered.connect(lambda: self.main._delete_via_hierarchy(item))
            menu.addAction(del_act)

            dup_act = QAction("Duplicate", self)
            dup_act.triggered.connect(lambda: self.main._duplicate_via_hierarchy(item))
            menu.addAction(dup_act)

            ren_act = QAction("Rename", self)
            ren_act.triggered.connect(lambda: self.tree.editItem(item, 0))
            menu.addAction(ren_act)

            child_act = QAction("Create Empty Child", self)
            child_act.triggered.connect(lambda: self.main._create_child_node(item))
            menu.addAction(child_act)
        else:
            root_act = QAction("Create Empty Root", self)
            root_act.triggered.connect(self.main._create_empty)
            menu.addAction(root_act)

        menu.exec(self.tree.mapToGlobal(point))


# ----------------------------------------------------------------------
#   Dock‑окно инспектора
# ----------------------------------------------------------------------
class InspectorDock(QDockWidget):
    def __init__(self, parent: QMainWindow):
        super().__init__("Inspector", parent)
        self.editor = ComponentEditor()
        self.setWidget(self.editor)


# ----------------------------------------------------------------------
#   Dock‑окно проекта
# ----------------------------------------------------------------------
class ProjectDock(QDockWidget):
    def __init__(self, parent: QMainWindow):
        super().__init__("Project", parent)
        w = QWidget()
        l = QVBoxLayout(w)

        self.asset_list = QListWidget()
        self.asset_list.addItems(["Models", "Textures", "Materials", "Scenes"])
        l.addWidget(self.asset_list)

        import_btn = QPushButton("Import OBJ…")
        import_btn.clicked.connect(parent._import_obj)
        l.addWidget(import_btn)

        self.setWidget(w)


# ----------------------------------------------------------------------
#   Dock‑окно консоли
# ----------------------------------------------------------------------
class ConsoleDock(QDockWidget):
    def __init__(self, parent: QMainWindow):
        super().__init__("Console", parent)
        w = QWidget()
        l = QVBoxLayout(w)

        self.text = QTextEdit()
        self.text.setReadOnly(True)
        self.text.setPlainText("AlKAsH3D Editor started…\n")
        l.addWidget(self.text)

        clear_btn = QPushButton("Clear")
        clear_btn.clicked.connect(self.text.clear)
        l.addWidget(clear_btn)

        self.setWidget(w)


# ----------------------------------------------------------------------
#   Главное окно редактора
# ----------------------------------------------------------------------
class MainWindow(QMainWindow):
    """Главное окно редактора со всеми панелями и логикой."""

    def __init__(self):
        super().__init__()
        self.setWindowTitle("AlKAsH3D Editor - Untitled")
        self.resize(1400, 800)

        # ───── Тема
        EditorTheme.apply_dark_theme(QApplication.instance())

        # ───── Сцена и камера
        self.scene = Scene()
        self.camera = Camera()
        self.camera.position = Vec3(0, 2, 5)
        self.scene.add_child(self.camera)

        self.transform_mode = TransformMode.TRANSLATE
        self.selected_node: Node | None = None

        # ───── UI (важный порядок!)
        #   1️⃣ меню, тул‑бар и центральный виджет
        #   2️⃣ статус‑бар (создаёт self.obj_info)
        #   3️⃣ док‑окна
        self._create_menu_bar()
        self._create_toolbars()
        self._create_central_widget()
        self._create_status_bar()      # ← ДО док‑окон
        self._create_docks()

        # ───── Таймеры
        self._setup_timers()

        # Сигналы от GL‑виджета
        self.gl_widget.object_selected.connect(self._on_gl_object_selected)
        self.gl_widget.add_mesh_requested.connect(self._add_mesh_at_position)

        # Undo/Redo
        self._undo_stack: list[dict] = []
        self._redo_stack: list[dict] = []

    # ------------------------------------------------------------------
    #   Меню
    # ------------------------------------------------------------------
    def _create_menu_bar(self):
        mb = self.menuBar()

        # ----- File -------------------------------------------------
        file = mb.addMenu("&File")
        new = QAction("New Scene", self)
        new.setShortcut(QKeySequence.New)
        new.triggered.connect(self._new_scene)
        file.addAction(new)

        open_ = QAction("Open Scene…", self)
        open_.setShortcut(QKeySequence.Open)
        open_.triggered.connect(self._open_scene)
        file.addAction(open_)

        save = QAction("Save Scene", self)
        save.setShortcut(QKeySequence.Save)
        save.triggered.connect(self._save_scene)
        file.addAction(save)

        save_as = QAction("Save Scene As…", self)
        save_as.setShortcut(QKeySequence.SaveAs)
        save_as.triggered.connect(self._save_scene_as)
        file.addAction(save_as)

        file.addSeparator()
        exit_ = QAction("Exit", self)
        exit_.setShortcut("Alt+F4")
        exit_.triggered.connect(self.close)
        file.addAction(exit_)

        # ----- Edit -------------------------------------------------
        edit = mb.addMenu("&Edit")
        undo = QAction("Undo", self)
        undo.setShortcut(QKeySequence.Undo)
        undo.triggered.connect(self._undo)
        edit.addAction(undo)

        redo = QAction("Redo", self)
        redo.setShortcut(QKeySequence.Redo)
        redo.triggered.connect(self._redo)
        edit.addAction(redo)

        edit.addSeparator()
        edit.addAction("Cut")
        edit.addAction("Copy")
        edit.addAction("Paste")
        edit.addSeparator()
        dup = QAction("Duplicate", self)
        dup.triggered.connect(self._duplicate_selected)
        edit.addAction(dup)

        delete = QAction("Delete", self)
        delete.setShortcut(QKeySequence.Delete)
        delete.triggered.connect(self._delete_selected)
        edit.addAction(delete)

        # ----- GameObject -------------------------------------------
        go = mb.addMenu("&GameObject")
        go.addAction("Create Empty", self._create_empty)

        obj = go.addMenu("3D Object")
        obj.addAction("Cube", self._add_cube)
        obj.addAction("Sphere", self._add_sphere)
        obj.addAction("Plane", self._add_plane)

        light = go.addMenu("Light")
        light.addAction("Directional Light", self._add_dir_light)
        light.addAction("Point Light", self._add_point_light)
        light.addAction("Spot Light", self._add_spot_light)

        go.addAction("Camera", self._add_camera)

        # ----- View -------------------------------------------------
        view = mb.addMenu("&View")
        self.grid_act = QAction("Show Grid", self, checkable=True)
        self.grid_act.setChecked(True)
        self.grid_act.triggered.connect(self._toggle_grid)
        view.addAction(self.grid_act)

        self.gizmo_act = QAction("Show Gizmo", self, checkable=True)
        self.gizmo_act.setChecked(True)
        self.gizmo_act.triggered.connect(self._toggle_gizmo)
        view.addAction(self.gizmo_act)

        self.wire_act = QAction("Wireframe Mode", self, checkable=True)
        self.wire_act.setChecked(False)
        self.wire_act.triggered.connect(self._toggle_wireframe)
        view.addAction(self.wire_act)

        view.addSeparator()
        bg = QAction("Background Colour…", self)
        bg.triggered.connect(self._pick_background_color)
        view.addAction(bg)

        # ----- Window ------------------------------------------------
        win = mb.addMenu("&Window")
        win.addAction("Hierarchy", self._toggle_hierarchy)
        win.addAction("Inspector", self._toggle_inspector)
        win.addAction("Project", self._toggle_project)
        win.addAction("Console", self._toggle_console)

        # ----- Help --------------------------------------------------
        help_ = mb.addMenu("&Help")
        about = QAction("About", self)
        about.triggered.connect(self._show_about)
        help_.addAction(about)

    # ------------------------------------------------------------------
    #   Toolbar
    # ------------------------------------------------------------------
    def _create_toolbars(self):
        tb = QToolBar("Main Toolbar")
        self.addToolBar(tb)

        # ----- Transform tools (W/E/R) -----------------------------
        self.trans_move = QAction("Move (W)", self)
        self.trans_move.setCheckable(True)
        self.trans_move.setChecked(True)
        self.trans_move.triggered.connect(lambda: self._set_transform_mode(TransformMode.TRANSLATE))
        tb.addAction(self.trans_move)

        self.trans_rotate = QAction("Rotate (E)", self)
        self.trans_rotate.setCheckable(True)
        self.trans_rotate.triggered.connect(lambda: self._set_transform_mode(TransformMode.ROTATE))
        tb.addAction(self.trans_rotate)

        self.trans_scale = QAction("Scale (R)", self)
        self.trans_scale.setCheckable(True)
        self.trans_scale.triggered.connect(lambda: self._set_transform_mode(TransformMode.SCALE))
        tb.addAction(self.trans_scale)

        # exclusive group
        tg = QActionGroup(self)
        tg.setExclusive(True)
        tg.addAction(self.trans_move)
        tg.addAction(self.trans_rotate)
        tg.addAction(self.trans_scale)

        tb.addSeparator()

        # ----- Edit‑Mode -----------------------------------------
        self.edit_mode_act = QAction("Edit Mode", self)
        self.edit_mode_act.setCheckable(True)
        self.edit_mode_act.triggered.connect(self._toggle_edit_mode)
        tb.addAction(self.edit_mode_act)

        tb.addSeparator()

        # ----- Play / Pause / Stop -------------------------------
        self.play_act = QAction("▶ Play", self)
        self.play_act.triggered.connect(self._play)
        tb.addAction(self.play_act)

        self.pause_act = QAction("⏸ Pause", self)
        self.pause_act.setEnabled(False)
        self.pause_act.triggered.connect(self._pause)
        tb.addAction(self.pause_act)

        self.stop_act = QAction("⏹ Stop", self)
        self.stop_act.setEnabled(False)
        self.stop_act.triggered.connect(self._stop)
        tb.addAction(self.stop_act)

    # ------------------------------------------------------------------
    #   Центральный виджет (OpenGL‑canvas)
    # ------------------------------------------------------------------
    def _create_central_widget(self):
        central = QWidget()
        self.setCentralWidget(central)
        lay = QVBoxLayout(central)
        lay.setContentsMargins(0, 0, 0, 0)

        self.gl_widget = GLWidget(self.scene, self.camera)
        lay.addWidget(self.gl_widget)

    # ------------------------------------------------------------------
    #   Dock‑окна
    # ------------------------------------------------------------------
    def _create_docks(self):
        self.hierarchy = HierarchyDock(self)
        self.addDockWidget(Qt.LeftDockWidgetArea, self.hierarchy)

        self.inspector = InspectorDock(self)
        self.addDockWidget(Qt.RightDockWidgetArea, self.inspector)

        self.project = ProjectDock(self)
        self.addDockWidget(Qt.LeftDockWidgetArea, self.project)
        self.project.hide()

        self.console = ConsoleDock(self)
        self.addDockWidget(Qt.BottomDockWidgetArea, self.console)
        self.console.hide()

    # ------------------------------------------------------------------
    #   Строка состояния
    # ------------------------------------------------------------------
    def _create_status_bar(self):
        sb = QStatusBar()
        self.setStatusBar(sb)

        self.status_lbl = QLabel("Ready")
        sb.addWidget(self.status_lbl)

        self.sel_info = QLabel("No selection")
        sb.addPermanentWidget(self.sel_info)

        self.obj_info = QLabel("Objects: 0")
        sb.addPermanentWidget(self.obj_info)

        self.fps_lbl = QLabel("FPS: 0")
        sb.addPermanentWidget(self.fps_lbl)

    # ------------------------------------------------------------------
    #   Таймеры (обновление сцены, FPS‑счётчик)
    # ------------------------------------------------------------------
    def _setup_timers(self):
        self._last_time = time.time()
        self._frame_cnt = 0
        self._fps_acc = 0.0
        self._is_playing = False

        self._timer = QTimer(self)
        self._timer.timeout.connect(self._main_loop)
        self._timer.start(16)      # ~60 fps

    # ------------------------------------------------------------------
    #   Главный цикл (FPS‑счётчик, обновление сцены)
    # ------------------------------------------------------------------
    def _main_loop(self):
        now = time.time()
        dt = now - self._last_time
        self._last_time = now

        self._frame_cnt += 1
        self._fps_acc += dt
        if self._fps_acc >= 1.0:
            fps = self._frame_cnt / self._fps_acc
            self.fps_lbl.setText(f"FPS: {int(fps)}")
            self._frame_cnt = 0
            self._fps_acc = 0.0

        self.scene.update(dt)
        self.gl_widget.update()

    # ------------------------------------------------------------------
    #   Выбор/синхронизация узлов
    # ------------------------------------------------------------------
    def _select_node(self, node: Node | None):
        self.selected_node = node
        self.inspector.editor.set_node(node)
        self.sel_info.setText(f"Selected: {node.name if node else 'None'}")
        self._log(f"Selected: {node.name if node else 'None'}")

    def _on_gl_object_selected(self, node):
        """Синхронизировать выбор, сделанный в GL‑виджете."""
        self._select_node(node)

        if node:
            def walk(it: QTreeWidgetItem):
                if it.data(0, Qt.UserRole) is node:
                    return it
                for i in range(it.childCount()):
                    res = walk(it.child(i))
                    if res:
                        return res
                return None

            root = self.hierarchy.tree.invisibleRootItem()
            for i in range(root.childCount()):
                found = walk(root.child(i))
                if found:
                    self.hierarchy.tree.setCurrentItem(found)
                    break

    # ------------------------------------------------------------------
    #   Логирование в консоль
    # ------------------------------------------------------------------
    def _log(self, text: str):
        self.console.text.append(f"[{time.strftime('%H:%M:%S')}] {text}")

    # ------------------------------------------------------------------
    #   Файловые операции (New, Open, Save, Save As)
    # ------------------------------------------------------------------
    def _new_scene(self):
        if QMessageBox.question(
                self, "New Scene",
                "Unsaved changes will be lost. Continue?",
                QMessageBox.Yes | QMessageBox.No) != QMessageBox.Yes:
            return

        self.scene = Scene()
        self.camera = Camera()
        self.camera.position = Vec3(0, 2, 5)
        self.scene.add_child(self.camera)

        self.gl_widget.scene = self.scene
        self.gl_widget.camera = self.camera
        self._select_node(None)
        self.hierarchy.refresh()
        self.setWindowTitle("AlKAsH3D Editor - Untitled")
        self._log("Created new scene")
        self._push_undo({"type": "new_scene", "scene": self.scene})

    def _open_scene(self):
        path, _ = QFileDialog.getOpenFileName(
                self, "Open Scene", "", "Scene Files (*.json)")
        if not path:
            return
        try:
            loaded = load_scene(path)
            if not isinstance(loaded, Scene):
                wrapper = Scene()
                wrapper.add_child(loaded)
                loaded = wrapper

            self.scene = loaded
            self.gl_widget.scene = self.scene
            self._select_node(None)
            self.hierarchy.refresh()
            self.setWindowTitle(f"AlKAsH3D Editor - {Path(path).name}")
            self._log(f"Loaded scene: {path}")
            self._push_undo({"type": "load", "path": path, "scene": self.scene})
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to load scene:\n{e}")
            self._log(f"ERROR loading: {e}")

    def _save_scene(self):
        self._save_scene_as()

    def _save_scene_as(self):
        path, _ = QFileDialog.getSaveFileName(
                self, "Save Scene", "", "Scene Files (*.json)")
        if not path:
            return
        try:
            save_scene(self.scene, path)
            self.setWindowTitle(f"AlKAsH3D Editor - {Path(path).name}")
            QMessageBox.information(self, "Success", "Scene saved.")
            self._log(f"Saved scene: {path}")
            self._push_undo({"type": "save", "path": path})
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to save scene:\n{e}")
            self._log(f"ERROR saving: {e}")

    # ------------------------------------------------------------------
    #   Создание базовых объектов
    # ------------------------------------------------------------------
    def _create_empty(self):
        empty = Node("GameObject")
        self.scene.add_child(empty)
        self.hierarchy.refresh()
        self._log("Created empty GameObject")
        self._push_undo({"type": "add_node", "node": empty,
                         "parent": self.scene})

    def _add_cube(self):
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
        mesh.position = Vec3(0, 0, 0)
        self.scene.add_child(mesh)
        self.hierarchy.refresh()
        self._log("Created Cube")
        self._push_undo({"type": "add_node", "node": mesh,
                         "parent": self.scene})

    def _add_sphere(self):
        mesh = Mesh(name="Sphere")      # placeholder
        self.scene.add_child(mesh)
        self.hierarchy.refresh()
        self._log("Created Sphere (placeholder)")
        self._push_undo({"type": "add_node", "node": mesh,
                         "parent": self.scene})

    def _add_plane(self):
        verts = np.array([
            [-5, 0, -5], [5, 0, -5],
            [5, 0, 5],  [-5, 0, 5]
        ], dtype=np.float32)

        indices = np.array([0,1,2, 2,3,0], dtype=np.uint32)

        mesh = Mesh(verts, indices=indices, name="Plane")
        self.scene.add_child(mesh)
        self.hierarchy.refresh()
        self._log("Created Plane")
        self._push_undo({"type": "add_node", "node": mesh,
                         "parent": self.scene})

    def _add_dir_light(self):
        light = DirectionalLight()
        light.name = "Directional Light"
        self.scene.add_child(light)
        self.hierarchy.refresh()
        self._log("Created Directional Light")
        self._push_undo({"type": "add_node", "node": light,
                         "parent": self.scene})

    def _add_point_light(self):
        light = PointLight()
        light.name = "Point Light"
        self.scene.add_child(light)
        self.hierarchy.refresh()
        self._log("Created Point Light")
        self._push_undo({"type": "add_node", "node": light,
                         "parent": self.scene})

    def _add_spot_light(self):
        light = SpotLight()
        light.name = "Spot Light"
        self.scene.add_child(light)
        self.hierarchy.refresh()
        self._log("Created Spot Light")
        self._push_undo({"type": "add_node", "node": light,
                         "parent": self.scene})

    def _add_camera(self):
        cam = Camera()
        cam.name = "Camera"
        cam.position = Vec3(0, 1, -3)
        self.scene.add_child(cam)
        self.hierarchy.refresh()
        self._log("Created Camera")
        self._push_undo({"type": "add_node", "node": cam,
                         "parent": self.scene})

    # ------------------------------------------------------------------
    #   Импорт OBJ
    # ------------------------------------------------------------------
    def _import_obj(self):
        path, _ = QFileDialog.getOpenFileName(
                self, "Import OBJ", "", "Wavefront OBJ (*.obj)")
        if not path:
            return
        try:
            pos, norm, tex, inds = load_obj(path)
            mesh = Mesh(pos, norm, tex, inds, name=Path(path).stem)
            self.scene.add_child(mesh)
            self.hierarchy.refresh()
            self._log(f"Imported OBJ: {path}")
            self._push_undo({"type": "add_node", "node": mesh,
                             "parent": self.scene})
        except Exception as e:
            QMessageBox.critical(self, "Import Error",
                                 f"Failed to import OBJ:\n{e}")
            self._log(f"ERROR importing OBJ: {e}")

    # ------------------------------------------------------------------
    #   Добавление Mesh по правому‑клику (запрос от GLWidget)
    # ------------------------------------------------------------------
    def _add_mesh_at_position(self, mesh: Mesh):
        self.scene.add_child(mesh)
        self.hierarchy.refresh()
        self._log(f"Added {mesh.name} at {mesh.position}")
        self._push_undo({"type": "add_node", "node": mesh,
                         "parent": self.scene})

    # ------------------------------------------------------------------
    #   Удаление / Дублирование (UI‑только)
    # ------------------------------------------------------------------
    def _delete_selected(self):
        node = self.selected_node
        if node and node is not self.scene:
            if node.parent:
                idx = node.parent.children.index(node)
                name = node.name
                node.parent.remove_child(node)
                self._log(f"Deleted: {name}")
                self._push_undo({"type": "delete_node",
                                 "node": node,
                                 "parent": node.parent,
                                 "index": idx})
                self._select_node(None)
                self.hierarchy.refresh()
        else:
            QMessageBox.warning(self, "Cannot Delete",
                                "No object selected or trying to delete root.")

    def _duplicate_selected(self):
        if not self.selected_node:
            return
        copy = dict_to_node(node_to_dict(self.selected_node))
        copy.name = f"{self.selected_node.name}_copy"
        if self.selected_node.parent:
            self.selected_node.parent.add_child(copy)
            self._push_undo({"type": "add_node", "node": copy,
                             "parent": self.selected_node.parent})
        else:
            self.scene.add_child(copy)
            self._push_undo({"type": "add_node", "node": copy,
                             "parent": self.scene})
        self.hierarchy.refresh()
        self._log(f"Duplicated: {self.selected_node.name}")

    # ------------------------------------------------------------------
    #   Delete/Duplicate через контекстное меню иерархии
    # ------------------------------------------------------------------
    def _delete_via_hierarchy(self, item: QTreeWidgetItem):
        node: Node = item.data(0, Qt.UserRole)
        if node is self.scene:
            QMessageBox.warning(self, "Cannot Delete", "Root scene cannot be deleted.")
            return
        parent = node.parent
        if parent:
            idx = parent.children.index(node)
            name = node.name
            parent.remove_child(node)
            self._log(f"Deleted (hierarchy): {name}")
            self._push_undo({"type": "delete_node",
                             "node": node,
                             "parent": parent,
                             "index": idx})
            self.hierarchy.refresh()
            if self.selected_node is node:
                self._select_node(None)

    def _duplicate_via_hierarchy(self, item: QTreeWidgetItem):
        node: Node = item.data(0, Qt.UserRole)
        copy = dict_to_node(node_to_dict(node))
        copy.name = f"{node.name}_copy"
        if node.parent:
            node.parent.add_child(copy)
        else:
            self.scene.add_child(copy)
        self.hierarchy.refresh()
        self._log(f"Duplicated (hierarchy): {node.name}")

    def _create_child_node(self, item: QTreeWidgetItem):
        parent: Node = item.data(0, Qt.UserRole)
        child = Node("GameObject")
        parent.add_child(child)
        self.hierarchy.refresh()
        self._log("Created child GameObject")

    # ------------------------------------------------------------------
    #   Play / Pause / Stop
    # ------------------------------------------------------------------
    def _play(self):
        self._is_playing = True
        self.play_act.setEnabled(False)
        self.pause_act.setEnabled(True)
        self.stop_act.setEnabled(True)
        self._log("Play mode started")

    def _pause(self):
        self._is_playing = False
        self.play_act.setEnabled(True)
        self.pause_act.setEnabled(False)
        self._log("Play mode paused")

    def _stop(self):
        self._is_playing = False
        self.play_act.setEnabled(True)
        self.pause_act.setEnabled(False)
        self.stop_act.setEnabled(False)
        self._log("Play mode stopped")

    # ------------------------------------------------------------------
    #   Трансформ‑мод (W/E/R)
    # ------------------------------------------------------------------
    def _set_transform_mode(self, mode: TransformMode):
        self.transform_mode = mode
        self.gl_widget.set_transform_mode(mode)
        names = {TransformMode.TRANSLATE: "Translate",
                 TransformMode.ROTATE:    "Rotate",
                 TransformMode.SCALE:     "Scale"}
        self._log(f"Transform mode: {names[mode]}")

    # ------------------------------------------------------------------
    #   Edit‑Mode (Tab)
    # ------------------------------------------------------------------
    def _toggle_edit_mode(self):
        new_state = not self.gl_widget._edit_mode
        self.gl_widget.set_edit_mode(new_state)
        self.edit_mode_act.setChecked(new_state)
        self._log(f"Edit Mode {'ON' if new_state else 'OFF'}")

    # ------------------------------------------------------------------
    #   Видимость док‑окон
    # ------------------------------------------------------------------
    def _toggle_hierarchy(self):
        self.hierarchy.setVisible(not self.hierarchy.isVisible())

    def _toggle_inspector(self):
        self.inspector.setVisible(not self.inspector.isVisible())

    def _toggle_project(self):
        self.project.setVisible(not self.project.isVisible())

    def _toggle_console(self):
        self.console.setVisible(not self.console.isVisible())

    # ------------------------------------------------------------------
    #   Показ/скрытие Grid / Gizmo / Wireframe / фон
    # ------------------------------------------------------------------
    def _toggle_grid(self):
        self.gl_widget.toggle_grid(not self.gl_widget._grid_visible)
        self.grid_act.setChecked(self.gl_widget._grid_visible)

    def _toggle_gizmo(self):
        self.gl_widget.toggle_gizmo(not self.gl_widget._gizmo_visible)
        self.gizmo_act.setChecked(self.gl_widget._gizmo_visible)

    def _toggle_wireframe(self):
        self.gl_widget.toggle_wireframe(not self.gl_widget._wireframe)
        self.wire_act.setChecked(self.gl_widget._wireframe)

    def _pick_background_color(self):
        col = QColorDialog.getColor(parent=self)
        if col.isValid():
            self.gl_widget.set_background_color(col.redF(),
                                              col.greenF(),
                                              col.blueF(),
                                              col.alphaF())

    # ------------------------------------------------------------------
    #   Информация о сцене (кол‑во объектов)
    # ------------------------------------------------------------------
    def _update_scene_info(self):
        """Обновить надпись «Objects: N» в статус‑баре."""
        cnt = self._count_objects(self.scene)
        if hasattr(self, "obj_info"):               # защита от преждевременного вызова
            self.obj_info.setText(f"Objects: {cnt}")

    def _count_objects(self, node: Node) -> int:
        total = 1
        for ch in node.children:
            total += self._count_objects(ch)
        return total

    # ------------------------------------------------------------------
    #   Undo / Redo (простейший стек)
    # ------------------------------------------------------------------
    def _push_undo(self, command: dict):
        self._undo_stack.append(command)
        self._redo_stack.clear()

    def _undo(self):
        if not self._undo_stack:
            return
        cmd = self._undo_stack.pop()
        self._apply_undo(cmd)
        self._redo_stack.append(cmd)

    def _redo(self):
        if not self._redo_stack:
            return
        cmd = self._redo_stack.pop()
        self._apply_redo(cmd)
        self._undo_stack.append(cmd)

    def _apply_undo(self, cmd: dict):
        typ = cmd["type"]
        if typ == "add_node":
            node = cmd["node"]
            parent = cmd["parent"]
            parent.remove_child(node)
        elif typ == "delete_node":
            node = cmd["node"]
            parent = cmd["parent"]
            idx = cmd["index"]
            parent.children.insert(idx, node)
            node.parent = parent
        self.hierarchy.refresh()
        self._log(f"Undo: {typ}")

    def _apply_redo(self, cmd: dict):
        typ = cmd["type"]
        if typ == "add_node":
            node = cmd["node"]
            parent = cmd["parent"]
            parent.add_child(node)
        elif typ == "delete_node":
            node = cmd["node"]
            parent = cmd["parent"]
            parent.remove_child(node)
        self.hierarchy.refresh()
        self._log(f"Redo: {typ}")

    # ------------------------------------------------------------------
    #   Выход
    # ------------------------------------------------------------------
    def closeEvent(self, event):
        if QMessageBox.question(
                self, "Exit",
                "Unsaved changes will be lost. Exit?",
                QMessageBox.Yes | QMessageBox.No) == QMessageBox.Yes:
            self._timer.stop()
            event.accept()
        else:
            event.ignore()

    # ------------------------------------------------------------------
    #   About
    # ------------------------------------------------------------------
    def _show_about(self):
        QMessageBox.about(self, "About AlKAsH3D Editor",
                          "AlKAsH3D Editor v1.0\n"
                          "A Unity/Blender‑like editor built with PySide6.\n"
                          "© 2026 AlKAsH3D Team")