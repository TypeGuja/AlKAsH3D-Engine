"""
Минимальный logger + (опциональная) DirectX 12 debug‑callback.
"""

import logging
import sys

def init_logger():
    logger = logging.getLogger("AlKAsH3D")
    logger.setLevel(logging.INFO)

    handler = logging.StreamHandler(sys.stdout)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s",
                            "%H:%M:%S")
    handler.setFormatter(fmt)
    logger.addHandler(handler)

    return logger

logger = init_logger()

def gl_check_error(context: str = ""):
    """
    В DX12‑режиме проверка OpenGL‑ошибок не требуется.
    Функция оставлена для совместимости – ничего не делает.
    """
    return
