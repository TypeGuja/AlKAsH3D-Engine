# -*- coding: utf-8 -*-
"""Простейший менеджер ввода."""
import glfw


class InputManager:
    def __init__(self, window):
        self.window = window
        self.keys = {}
        self.mouse = {"dx": 0, "dy": 0, "x": 0, "y": 0}
        self._setup_callbacks()

    def _setup_callbacks(self):
        import glfw
        glfw.set_key_callback(self.window, self._key_cb)
        glfw.set_cursor_pos_callback(self.window, self._mouse_move_cb)
        glfw.set_input_mode(self.window, glfw.CURSOR, glfw.CURSOR_DISABLED)

    def _key_cb(self, win, key, scancode, action, mods):
        self.keys[key] = action != glfw.RELEASE

    def _mouse_move_cb(self, win, xpos, ypos):
        dx = xpos - self.mouse["x"]
        dy = ypos - self.mouse["y"]
        self.mouse.update({"dx": dx, "dy": dy, "x": xpos, "y": ypos})

    def is_key_pressed(self, key):
        return self.keys.get(key, False)

    def get_mouse_delta(self):
        dx, dy = self.mouse["dx"], self.mouse["dy"]
        self.mouse["dx"], self.mouse["dy"] = 0, 0
        return dx, dy
