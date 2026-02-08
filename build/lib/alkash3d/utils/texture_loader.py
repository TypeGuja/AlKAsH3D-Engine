# alkash3d/utils/texture_loader.py
from PIL import Image
from OpenGL import GL
import numpy as np

def load_texture(path):
    """Загружает PNG/JPG → OpenGL‑текстуру, возвращает GLuint."""
    img = Image.open(path).convert("RGBA")
    img_data = np.array(img, dtype=np.uint8)
    w, h = img.size

    tex_id = GL.glGenTextures(1)
    GL.glBindTexture(GL.GL_TEXTURE_2D, tex_id)
    GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGBA8,
                    w, h, 0,
                    GL.GL_RGBA, GL.GL_UNSIGNED_BYTE, img_data)

    # опции фильтрации и мипмэппинг
    GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_LINEAR_MIPMAP_LINEAR)
    GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_LINEAR)
    GL.glGenerateMipmap(GL.GL_TEXTURE_2D)
    return tex_id
