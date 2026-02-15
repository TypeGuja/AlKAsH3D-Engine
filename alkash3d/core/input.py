"""
Скрывает GLFW‑callback‑механику.
"""

import glfw

class InputManager:
    """Скрывает GLFW‑callback‑механику."""
    def __init__(self, window):
        self.window = window
        self.keys = {}
        self.mouse = {"dx": 0.0, "dy": 0.0, "x": 0.0, "y": 0.0}
        self.scroll = {"dx": 0.0, "dy": 0.0}
        self._setup_callbacks()

    def _setup_callbacks(self):
        glfw.set_key_callback(self.window, self._key_cb)
        glfw.set_cursor_pos_callback(self.window, self._mouse_move_cb)
        glfw.set_scroll_callback(self.window, self._mouse_scroll_cb)
        glfw.set_input_mode(self.window, glfw.CURSOR, glfw.CURSOR_DISABLED)

    def _key_cb(self, win, key, scancode, action, mods):
        self.keys[key] = action != glfw.RELEASE

    def _mouse_move_cb(self, win, xpos, ypos):
        dx = xpos - self.mouse["x"]
        dy = ypos - self.mouse["y"]
        self.mouse.update({"dx": dx, "dy": dy, "x": xpos, "y": ypos})

    def _mouse_scroll_cb(self, win, xoff, yoff):
        self.scroll["dx"] += xoff
        self.scroll["dy"] += yoff

    def is_key_pressed(self, key) -> bool:
        return self.keys.get(key, False)

    def get_mouse_delta(self):
        dx, dy = self.mouse["dx"], self.mouse["dy"]
        self.mouse["dx"], self.mouse["dy"] = 0.0, 0.0
        return dx, dy

    def get_scroll_delta(self):
        dx, dy = self.scroll["dx"], self.scroll["dy"]
        self.scroll["dx"], self.scroll["dy"] = 0.0, 0.0
        return dx, dy