# alkas3d/utils/logger.py
# ---------------------------------------------------------------
# Минимальный логгер + OpenGL‑сообщения для отладки.
# ---------------------------------------------------------------

import logging
from OpenGL import GL

def init_logger():
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    return logging.getLogger("AlKAsH3D")

logger = init_logger()

def gl_check_error(context: str = ""):
    """Проверить glGetError и вывести в лог, если что‑то не так."""
    err = GL.glGetError()
    if err != GL.GL_NO_ERROR:
        msg = GL.gluErrorString(err).decode()
        logger.error(f"OpenGL error {msg} [{context}]")
