#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
unittest‑тест для DX12‑бэкенда.

После всех исправлений он проходит без «access‑violation» и без
сообщений уровня ERROR/CRITICAL (в частности – без 0x80070057).
"""

from __future__ import annotations

import unittest
import logging
import numpy as np

# ----------------------------------------------------------------------
# 1️⃣ Лог‑перехватчик (ERROR/CRITICAL)
# ----------------------------------------------------------------------
class LogCaptureHandler(logging.Handler):
    """Запоминает только сообщения ERROR/CRITICAL."""
    def __init__(self):
        super().__init__(level=logging.ERROR)
        self.records: list[logging.LogRecord] = []

    def emit(self, record: logging.LogRecord) -> None:
        self.records.append(record)


# ----------------------------------------------------------------------
# 2️⃣ Тестовый класс
# ----------------------------------------------------------------------
class TestDX12Backend(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        """Создаём скрытое окно и инициализируем DX12‑backend."""
        # 2.1 скрытое окно (только HWND)
        from alkash3d.window import Window
        import glfw
        cls.win = Window(2, 2, "AlKAsH3D‑Test‑Wnd")
        glfw.hide_window(cls.win.handle)               # скрыть, чтобы не мигал

        # 2.2 DX12‑backend
        from alkash3d.graphics import select_backend
        cls.backend = select_backend("dx12")
        cls.backend.init_device(cls.win.hwnd, cls.win.width, cls.win.height)

        # 2.3 Лог‑перехват
        cls.log_handler = LogCaptureHandler()
        logging.getLogger("AlKAsH3D").addHandler(cls.log_handler)

    @classmethod
    def tearDownClass(cls):
        """Очищаем ресурсы."""
        try:
            cls.backend.shutdown()
        except Exception:
            # Возможные двойные free‑операции уже защищены внутри shutdown,
            # но на всякий случай игнорируем любые исключения.
            pass

        try:
            cls.win.close()
        except Exception:
            pass

        logging.getLogger("AlKAsH3D").removeHandler(cls.log_handler)

    # ------------------------------------------------------------------
    # 3️⃣ Тест‑метод: полный путь от CBV → placeholder‑texture → draw → present
    # ------------------------------------------------------------------
    def test_full_dx12_path(self):
        # --------------------------------------------------------------
        # 3.1 Константный буфер (CBV)
        # --------------------------------------------------------------
        dummy_data = b"\x00" * 256          # 256 байт, уже выровнено
        cbv, cbv_gpu = self.backend.create_constant_buffer(dummy_data)
        self.assertIsNotNone(cbv, "create_constant_buffer вернул None")
        self.assertNotEqual(cbv_gpu, 0, "GPU‑handle CBV = 0")
        # повторно обновляем, чтобы убедиться, что update не падает
        self.backend.update_buffer(cbv, dummy_data)

        # --------------------------------------------------------------
        # 3.2 Placeholder‑текстура 1×1 (белый пиксель)
        # --------------------------------------------------------------
        white_pixel = (255).to_bytes(1, "little") * 4   # RGBA(255,255,255,255)
        tex = self.backend.create_texture(
            data=white_pixel,
            w=1,
            h=1,
            fmt="RGBA8",
        )
        self.assertIsNotNone(tex, "create_texture (placeholder) вернул None")
        self.assertTrue(
            hasattr(tex, "_srv_gpu") and tex._srv_gpu != 0,
            "placeholder‑текстура должна иметь валидный SRV‑handle"
        )

        # --------------------------------------------------------------
        # 3.3 RTV‑дескриптор для back‑buffer‑а (swap‑chain)
        # --------------------------------------------------------------
        rtv0 = self.backend.rtv_heap.get_cpu_handle(0)
        self.assertIsInstance(rtv0, int, "RTV‑handle должен быть int")
        self.backend.set_render_target(rtv0)

        # --------------------------------------------------------------
        # 3.4 Простейший vertex‑/index‑буфер (треугольник)
        # --------------------------------------------------------------
        verts = np.array(
            [ -0.5, -0.5, 0.0,
               0.5, -0.5, 0.0,
               0.0,  0.5, 0.0 ], dtype=np.float32
        )
        inds = np.array([0, 1, 2], dtype=np.uint32)

        vb = self.backend.create_buffer(verts.tobytes(), usage="vertex")
        ib = self.backend.create_buffer(inds.tobytes(), usage="index")
        self.backend.set_vertex_buffers(vb, ib)

        # --------------------------------------------------------------
        # 3.5 Один draw‑call
        # --------------------------------------------------------------
        self.backend.draw_indexed(
            index_count=3,
            start_index=0,
            base_vertex=0,
            instance_count=1,
        )

        # --------------------------------------------------------------
        # 3.6 Present (один кадр)
        # --------------------------------------------------------------
        self.backend.present()                # внутри – sync_interval = 0
        self.backend.wait_for_gpu()          # гарантируем завершение GPU‑команд

        # --------------------------------------------------------------
        # 3.7 Проверяем, что ни один ERROR/CRITICAL не появился в журнале
        # --------------------------------------------------------------
        errors = [r.getMessage() for r in self.log_handler.records]
        if errors:
            self.fail(
                "В логах движка зафиксированы ERROR/CRITICAL:\n"
                + "\n".join(f" • {msg}" for msg in errors)
            )


# ----------------------------------------------------------------------
# Позволяем запускать файл напрямую (удобно для отладки)
# ----------------------------------------------------------------------
if __name__ == "__main__":
    unittest.main(verbosity=2)