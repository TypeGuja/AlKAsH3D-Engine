# alkash3d_rtx/__init__.py
# -*- coding: utf-8 -*-
"""
Минимальный RTX‑модуль для AlKAsH3D.

Главная публичная функция – render_frame, которая принимает JSON‑строку
и размеры кадра, а дальше делегирует всё _kernel.render_image.
"""

from __future__ import annotations

import json
from typing import Tuple

# Импортируем наш kernel‑модуль
from ._kernel import render_image

__all__ = ("render_frame",)


def _extract_camera(payload: dict) -> Tuple[Tuple[float, float, float],
                                          Tuple[float, float, float],
                                          Tuple[float, float, float]]:
    """
    Возврат (pos, target, up) – берём их из JSON, если они есть.
    Если поля отсутствуют – используем «заглушку».
    """
    cam = payload.get("camera", {})
    pos = tuple(cam.get("position", (0.0, 0.0, 5.0)))
    target = tuple(cam.get("target", (0.0, 0.0, 0.0)))
    up = tuple(cam.get("up", (0.0, 1.0, 0.0)))
    return pos, target, up


def render_frame(scene_json: str, width: int, height: int) -> bytes:
    """
    API, ожидаемое движком RTXRenderer.

    * scene_json – произвольный JSON.  В текущей реализации
      используется только информация о камере (position/target/up).
    * width, height – размеры изображения.

    Возврат – байтовый RGBA‑буфер, готовый к загрузке в DX12‑текстуру.
    """
    # -----------------------------------------------------------------
    # 1️⃣ Пытаемся распарсить JSON, но любые ошибки игнорируем –
    #    будем использовать камеру‑заглушку.
    # -----------------------------------------------------------------
    try:
        payload = json.loads(scene_json)
        cam_pos, cam_target, cam_up = _extract_camera(payload)
    except Exception:
        cam_pos, cam_target, cam_up = (0.0, 0.0, 5.0), (0.0, 0.0, 0.0), (0.0, 1.0, 0.0)

    # -----------------------------------------------------------------
    # 2️⃣ Делегируем рендеринг ядру
    # -----------------------------------------------------------------
    return render_image(width, height, cam_pos, cam_target, cam_up)