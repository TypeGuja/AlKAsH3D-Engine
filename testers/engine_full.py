# -*- coding: utf-8 -*-
"""
testers/engine_full.py

Тест «один кадр» для полного движка.
Используется мок‑бэкенд из `tests/conftest.py`.  Проверяем, что
Engine → Renderer → Backend вызывают все необходимые методы.
"""

import numpy as np
import pytest

from alkash3d.engine import Engine
from alkash3d.scene import Scene, Camera, Mesh
from alkash3d.renderer.pipelines.forward import ForwardRenderer


# ----------------------------------------------------------------------
# Минимальная имитация InputManager (не нужна в этом тесте)
# ----------------------------------------------------------------------
class _DummyInput:
    def is_key_pressed(self, key):          # noqa: D401
        """Никакие клавиши не нажаты."""
        return False

    def get_mouse_delta(self):
        return (0.0, 0.0)

    def get_scroll_delta(self):
        return (0.0, 0.0)


# ----------------------------------------------------------------------
# Окно‑заглушка.  Главное – метод ``resource_path`` (нужен Shader)
# ----------------------------------------------------------------------
class _DummyWindow:
    """
    Минимальная имитация ``alkash3d.window.Window``.
    Требуется только:
        * width, height, hwnd, title,
        * input (для Engine),
        * ``resource_path`` – возвращает абсолютный путь к файлам в папке
          ``resources/`` проекта.
    """
    def __init__(self):
        self.width = 640
        self.height = 480
        self.hwnd = 0               # 0 → без реального swap‑chain
        self.title = "dummy"
        self.input = _DummyInput()

    # Методы, вызываемые Engine (но ничего не делают)
    def set_vsync(self, _: bool): pass
    def poll_events(self): pass
    def should_close(self) -> bool: return False
    def swap_buffers(self): pass
    def close(self): pass

    # ------------------------------
    # Важный метод – путь к ресурсам
    # ------------------------------
    def resource_path(self, relative_path: str):
        """
        Возвращает абсолютный путь к файлу внутри ``resources/``.
        Тестовый файл находится в каталоге ``testers/``, поэтому
        поднимаемся на один уровень вверх до корня репозитория.
        """
        from pathlib import Path
        repo_root = Path(__file__).resolve().parents[1]   # …/AlKAsH3D-Engine
        return repo_root / "resources" / relative_path


def test_engine_one_frame(mock_backend):
    """
    Создаём Engine без реального окна, подменяем backend на MockBackend
    и отрисовываем один кадр через ForwardRenderer.
    После этого проверяем, что в мок‑бэкенде зафиксированы основные вызовы.
    """
    # -----------------------------------------------------------------
    # 1️⃣  Подменяем функцию select_backend → всегда возвращаем mock
    # -----------------------------------------------------------------
    import alkash3d.graphics as gfx
    gfx.select_backend = lambda _: mock_backend   # любой запрос → наш мок

    # -----------------------------------------------------------------
    # 2️⃣  Окно‑заглушка
    # -----------------------------------------------------------------
    dummy_win = _DummyWindow()

    # -----------------------------------------------------------------
    # 3️⃣  Конструируем Engine вручную (минуем обычный __init__)
    # -----------------------------------------------------------------
    engine = Engine.__new__(Engine)            # создаём «сырой» объект

    # Минимальная подстановка конфигурации (чтобы Engine не пытался
    # читать файл с диска).  Достаточно методов ``__getitem__``,
    # ``get`` и ``__setitem__``.
    engine.cfg = type(
        "FakeCfg",
        (),
        {
            "__getitem__": lambda s, k: None,
            "get": lambda s, k, d=None: None,
            "__setitem__": lambda s, k, v: None,
        },
    )()

    # Привязываем окно и мок‑бэкенд
    engine.window = dummy_win
    engine.backend = mock_backend
    engine.backend.init_device(dummy_win.hwnd, dummy_win.width, dummy_win.height)

    # -----------------------------------------------------------------
    # 4️⃣  Минимальная сцена: камера + один простой Mesh
    # -----------------------------------------------------------------
    scene = Scene()
    cam = Camera()
    scene.add_child(cam)

    # Плоский квадрат (чтобы отрисовка действительно вызвала draw)
    verts = np.array([[-1, -1, 0], [1, -1, 0],
                      [1, 1, 0], [-1, 1, 0]], dtype=np.float32)
    inds = np.array([0, 1, 2, 0, 2, 3], dtype=np.uint32)
    mesh = Mesh(verts, indices=inds, name="Quad")
    scene.add_child(mesh)

    engine.scene = scene
    engine.camera = cam

    # -----------------------------------------------------------------
    # 5️⃣  ForwardRenderer (использует наш мок‑бэкенд)
    # -----------------------------------------------------------------
    engine.renderer = ForwardRenderer(dummy_win, backend=mock_backend)

    # -----------------------------------------------------------------
    # 6️⃣  Выполняем ровно один кадр (ручной вызов render)
    # -----------------------------------------------------------------
    engine.renderer.render(engine.scene, engine.camera)

    # -----------------------------------------------------------------
    # 7️⃣  Проверяем, что в мок‑бэкенде зафиксированы основные шаги
    # -----------------------------------------------------------------
    # 7.1  Начало/конец кадра
    assert mock_backend.called("begin_frame")
    assert mock_backend.called("end_frame")

    # 7.2  Очистка render‑target (RTV0)
    assert mock_backend.called("clear_render_target")

    # 7.3  Должен был выполнен хотя бы один draw‑кол (indexed или обычный)
    assert mock_backend.called("draw") or mock_backend.called("draw_indexed")

    # 7.4  После рендера вызывается present (в end_frame)
    assert mock_backend.called("present")

    # -----------------------------------------------------------------
    # 8️⃣  Завершаем тест – вызываем shutdown и проверяем, что он не падает
    # -----------------------------------------------------------------
    # Engine.shutdown ожидает атрибут ``postprocess``; в нашем «ручном» объекте
    # он не создавался, поэтому устанавливаем заглушку.
    engine.postprocess = None
    engine.shutdown()
