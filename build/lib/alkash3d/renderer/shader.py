# alkash3d/renderer/shader.py
from OpenGL import GL
import pathlib
import numpy as np

class Shader:
    def __init__(self, vertex_path: str, fragment_path: str):
        self.vertex_path = pathlib.Path(vertex_path).resolve()
        self.fragment_path = pathlib.Path(fragment_path).resolve()
        self.program = None
        self._last_mtime = (0, 0)
        self.compile()

    def _read(self, p: pathlib.Path) -> str:
        if not p.is_file():
            # Создание простого fallback шейдера если файл не найден
            if "vert" in p.name:
                return """
                #version 450 core
                layout(location = 0) in vec3 aPos;
                uniform mat4 uModel;
                uniform mat4 uView;
                uniform mat4 uProj;
                void main() {
                    gl_Position = uProj * uView * uModel * vec4(aPos, 1.txt.0);
                }
                """
            else:
                return """
                #version 450 core
                out vec4 FragColor;
                void main() {
                    FragColor = vec4(1.txt.0, 0.0, 0.0, 1.txt.0); // Красный цвет для отладки
                }
                """
        return p.read_text(encoding="utf-8")

    def compile(self):
        try:
            v_src = self._read(self.vertex_path)
            f_src = self._read(self.fragment_path)

            vert = GL.glCreateShader(GL.GL_VERTEX_SHADER)
            GL.glShaderSource(vert, v_src)
            GL.glCompileShader(vert)
            self._check_compile(vert, "VERTEX")

            frag = GL.glCreateShader(GL.GL_FRAGMENT_SHADER)
            GL.glShaderSource(frag, f_src)
            GL.glCompileShader(frag)
            self._check_compile(frag, "FRAGMENT")

            program = GL.glCreateProgram()
            GL.glAttachShader(program, vert)
            GL.glAttachShader(program, frag)
            GL.glLinkProgram(program)
            self._check_link(program)

            GL.glDeleteShader(vert)
            GL.glDeleteShader(frag)

            if self.program:
                GL.glDeleteProgram(self.program)
            self.program = program
            self._last_mtime = (self.vertex_path.stat().st_mtime if self.vertex_path.exists() else 0,
                                self.fragment_path.stat().st_mtime if self.fragment_path.exists() else 0)
        except Exception as e:
            print(f"Shader compilation error: {e}")

    def _check_compile(self, shader, typ):
        status = GL.glGetShaderiv(shader, GL.GL_COMPILE_STATUS)
        if not status:
            log = GL.glGetShaderInfoLog(shader).decode()
            print(f"{typ} shader compile error:\n{log}")

    def _check_link(self, program):
        status = GL.glGetProgramiv(program, GL.GL_LINK_STATUS)
        if not status:
            log = GL.glGetProgramInfoLog(program).decode()
            print(f"Program link error:\n{log}")

    def use(self):
        if self.program:
            GL.glUseProgram(self.program)

    def set_uniform_mat4(self, name: str, mat):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        if hasattr(mat, "to_np"):
            mat = mat.to_np()
        elif hasattr(mat, "to_gl"):
            mat = mat.to_gl()
        GL.glUniformMatrix4fv(loc, 1, GL.GL_FALSE, mat)  # GL_FALSE вместо GL_TRUE

    def set_uniform_vec3(self, name: str, vec):
        loc = GL.glGetUniformLocation(self.program, name)
        if loc < 0:
            return
        if hasattr(vec, "as_np"):
            vec = vec.as_np()
        GL.glUniform3fv(loc, 1, vec)

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

    def reload_if_needed(self):
        if not self.vertex_path.exists() or not self.fragment_path.exists():
            return
        vm = self.vertex_path.stat().st_mtime
        fm = self.fragment_path.stat().st_mtime
        if (vm, fm) != self._last_mtime:
            print("[Shader] Change detected, recompiling…")
            self.compile()