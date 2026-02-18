#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Расширенный набор юнит‑тестов для DX12‑бэкенда.

Тест покрывает :
* создание placeholder‑текстуры без начальных данных и последующее обновление;
* работу с несколькими constant‑buffer‑ами (проверка, что повторный
  вызов `update_buffer` не приводит к AV);
* базовую проверку, что никаких сообщений уровня ERROR/CRITICAL не появляется
  в журнале движка.

Требования
-----------

* На тест-машине должна быть доступна DirectX 12 (работает в head‑less режиме,
  т.е. без реального окна — минимальное окно создаётся скрытым).
* Пакет `alkash3d` уже находится в `PYTHONPATH` (как в оригинальном проекте).

Запуск
-------

$ pytest -q test_engine_extended.py
"""

from __future__ import annotations

import unittest
import logging
import numpy as np
import glfw

# ----------------------------------------------------------------------
# 1️⃣ Лог‑перехватчик (ERROR/CRITICAL)
# ----------------------------------------------------------------------
class LogCaptureHandler(logging.Handler):
    """Запоминает только сообщения уровня ERROR/CRITICAL."""
    def __init__(self):
        super().__init__(level=logging.ERROR)
        self.records: list[logging.LogRecord] = []

    def emit(self, record: logging.LogRecord) -> None:
        self.records.append(record)

# ----------------------------------------------------------------------
# 2️⃣ Базовый набор вспомогательных функций
# ----------------------------------------------------------------------
def _make_hidden_window() -> "alkash3d.window.Window":
    """
    Создаёт скрытое окно (только HWND) и возвращает объект Window.
    """
    from alkash3d.window import Window
    win = Window(2, 2, "AlKAsH3D‑Test‑Wnd")
    # Спрячем окно, чтобы не мигал в процессе тестов
    glfw.hide_window(win.handle)
    return win


def _init_dx12_backend(win: "alkash3d.window.Window"):
    """Инициализирует DX12‑backend в режиме, пригодном для юнит‑тестов."""
    from alkash3d.graphics import select_backend
    backend = select_backend("dx12")
    backend.init_device(win.hwnd, win.width, win.height)
    return backend

# ----------------------------------------------------------------------
# 3️⃣ Тестовый набор
# ----------------------------------------------------------------------
class TestDX12BackendExtended(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        """Создаём скрытое окно, инициализируем DX12‑backend и подключаем лог‑перехватчик."""
        cls.win = _make_hidden_window()
        cls.backend = _init_dx12_backend(cls.win)

        cls.log_handler = LogCaptureHandler()
        logging.getLogger("AlKAsH3D").addHandler(cls.log_handler)

    @classmethod
    def tearDownClass(cls):
        """Закрываем backend и окно, убираем лог‑перехватчик."""
        try:
            cls.backend.shutdown()
        finally:
            # Гарантируем, что окно будет закрыто независимо от того,
            # удалось ли корректно завершить backend.
            try:
                cls.win.close()
            finally:
                pass

        logging.getLogger("AlKAsH3D").removeHandler(cls.log_handler)

    # ------------------------------------------------------------------
    # 3.1 Тест: placeholder‑текстура без начального содержимого,
    #      последующее обновление через update_texture().
    # ------------------------------------------------------------------
    def test_placeholder_texture_update(self):
        """
        1️⃣ Создаём texture без данных (DEFAULT‑heap).
        2️⃣ Обновляем её содержимое через `backend.update_texture`.
        3️⃣ Ожидаем отсутствие ERROR/CRITICAL в журнале.
        """
        # ── 1️⃣ без данных (это DEFAULT‑heap) ────────────────────────
        tex = self.backend.create_texture(
            data=None,          # без начального содержимого
            w=1,
            h=1,
            fmt="RGBA8",
        )
        self.assertIsNotNone(tex, "create_texture без data вернул None")
        self.assertTrue(
            hasattr(tex, "_srv_gpu") and tex._srv_gpu != 0,
            "SRV‑handle должен быть валидным даже для texture без данных",
        )

        # ── 2️⃣ загружаем один белый пиксель через отдельный вызов ───────
        white_pixel = (255).to_bytes(1, "little") * 4   # RGBA(255,255,255,255)
        # Вызываем отдельный апдейт – в текущей реализации используется
        # `dx.update_texture`, который в свою очередь делает Map/Copy.
        # На большинстве драйверов это проходит без ошибок (может выдать debug‑сообщение,
        # но не ERROR), что и проверяем ниже.
        self.backend.update_texture(tex, white_pixel, w=1, h=1)

        # ── 3️⃣ проверяем, что журнал чист (ERROR/CRITICAL) ─────────────
        errors = [rec.getMessage() for rec in self.log_handler.records]
        self.assertFalse(
            errors,
            "В журнале обнаружены сообщения уровня ERROR/CRITICAL:\n" + "\n".join(errors),
        )

    # ------------------------------------------------------------------
    # 3.2 Тест: несколько constant‑buffer‑ов, проверка повторного обновления.
    # ------------------------------------------------------------------
    def test_multiple_constant_buffers(self):
        """
        1️⃣ Создаём два const‑буфера разного содержания.
        2️⃣ Обновляем каждый буфер несколько раз – проверка,
           что `update_buffer` не падает (из‑за фильтрации CBV‑буферов).
        3️⃣ Убеждаемся, что оба CBV‑хендла получены и не нулевые.
        """
        data_a = b"\x01" * 256
        data_b = b"\x02" * 256

        cbv_a, gpu_a = self.backend.create_constant_buffer(data_a)
        cbv_b, gpu_b = self.backend.create_constant_buffer(data_b)

        self.assertIsNotNone(cbv_a, "Первый constant buffer не создан")
        self.assertIsNotNone(cbv_b, "Второй constant buffer не создан")
        self.assertNotEqual(gpu_a, 0, "GPU‑handle первого CBV равен 0")
        self.assertNotEqual(gpu_b, 0, "GPU‑handle второго CBV равен 0")
        self.assertNotEqual(gpu_a, gpu_b, "GPU‑handle двух разных CBV совпали")

        # Повторные обновления (должны быть безопасными)
        for _ in range(3):
            self.backend.update_buffer(cbv_a, data_a)
            self.backend.update_buffer(cbv_b, data_b)

        # Очистка ошибок в журнале
        errors = [rec.getMessage() for rec in self.log_handler.records]
        self.assertFalse(
            errors,
            "Во время работы с constant‑buffer‑ами появились ERROR/CRITICAL:\n"
            + "\n".join(errors),
        )

    # ------------------------------------------------------------------
    # 3.3 Тест: базовый draw‑pipeline, аналогичный оригинальному тесту.
    # ------------------------------------------------------------------
    def test_basic_draw_path(self):
        """
        Полный путь: CBV → placeholder‑texture → vertex/index‑буфер → draw → present.
        Проверяем, что команда отрисовки завершается без ошибок.
        """
        # CBV
        dummy = b"\x00" * 256
        cbv, gpu_cbv = self.backend.create_constant_buffer(dummy)
        self.assertIsNotNone(cbv)
        self.assertNotEqual(gpu_cbv, 0)

        # Placeholder‑texture (создаём без данных, потом апдейтим)
        tex = self.backend.create_texture(data=None, w=1, h=1, fmt="RGBA8")
        self.assertIsNotNone(tex)
        self.assertTrue(hasattr(tex, "_srv_gpu") and tex._srv_gpu != 0)
        self.backend.update_texture(tex, b"\xff\xff\xff\xff", w=1, h=1)

        # RTV back‑buffer
        rtv0 = self.backend.rtv_heap.get_cpu_handle(0)
        self.assertIsInstance(rtv0, int)
        self.backend.set_render_target(rtv0)

        # Vertex / index буферы (простой треугольник)
        verts = np.array(
            [-0.5, -0.5, 0.0,
             0.5, -0.5, 0.0,
             0.0,  0.5, 0.0], dtype=np.float32
        )
        inds  = np.array([0, 1, 2], dtype=np.uint32)

        vb = self.backend.create_buffer(verts.tobytes(), usage="vertex")
        ib = self.backend.create_buffer(inds.tobytes(), usage="index")
        self.backend.set_vertex_buffers(vb, ib)

        # Draw
        self.backend.draw_indexed(index_count=3, start_index=0, base_vertex=0, instance_count=1)

        # Present + GPU sync
        self.backend.present()
        self.backend.wait_for_gpu()

        # Проверка журнала
        errors = [rec.getMessage() for rec in self.log_handler.records]
        self.assertFalse(
            errors,
            "В процессе базового draw‑pipeline были обнаружены сообщения ERROR/CRITICAL:\n"
            + "\n".join(errors),
        )

# ----------------------------------------------------------------------
# Позволяем запускать файл напрямую (удобно для отладки)
# ----------------------------------------------------------------------
if __name__ == "__main__":
    unittest.main(verbosity=2)