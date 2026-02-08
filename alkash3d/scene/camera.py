# alkash3d/scene/camera.py
import glfw
import numpy as np
from alkash3d.scene.node import Node
from alkash3d.math.vec3 import Vec3
from alkash3d.math.mat4 import Mat4


class Camera(Node):
    def __init__(self, fov=60.0, near=0.1, far=1000.0, name="Camera"):
        super().__init__(name)
        self.fov = fov
        self.near = near
        self.far = far
        # Установка начальной позиции камеры
        self.position = Vec3(0, 0, 5)

    def get_view_matrix(self):
        """
        Исправленная версия - используем look_at вместо инвертирования
        """
        # Позиция камеры
        eye = self.position.as_np()

        # Направление взгляда (куда смотрим)
        target_pos = self.position + self.forward
        target = target_pos.as_np()

        # Вектор "вверх"
        up_vec = Vec3(0, 1, 0).as_np()

        return Mat4.look_at(eye, target, up_vec).to_np()

    def get_projection_matrix(self, aspect_ratio):
        return Mat4.perspective(self.fov, aspect_ratio, self.near, self.far).to_np()

    def update_fly(self, dt, input_manager):
        speed = 5.0 * dt
        rot_speed = 90.0 * dt

        if input_manager.is_key_pressed(glfw.KEY_W):
            self.position = self.position + self.forward * speed
        if input_manager.is_key_pressed(glfw.KEY_S):
            self.position = self.position - self.forward * speed
        if input_manager.is_key_pressed(glfw.KEY_A):
            self.position = self.position - self.right * speed
        if input_manager.is_key_pressed(glfw.KEY_D):
            self.position = self.position + self.right * speed
        if input_manager.is_key_pressed(glfw.KEY_SPACE):
            self.position = self.position + self.up * speed
        if input_manager.is_key_pressed(glfw.KEY_LEFT_SHIFT):
            self.position = self.position - self.up * speed

        dx, dy = input_manager.get_mouse_delta()
        self.rotation.y += dx * 0.1
        self.rotation.x += dy * 0.1
        self.rotation.x = max(-89.0, min(89.0, self.rotation.x))

    @property
    def forward(self):
        yaw = np.radians(self.rotation.y)
        pitch = np.radians(self.rotation.x)
        x = np.cos(pitch) * np.sin(yaw)
        y = np.sin(pitch)
        z = np.cos(pitch) * np.cos(yaw)
        return Vec3(x, y, -z).normalized()

    @property
    def right(self):
        return self.forward.cross(Vec3(0, 1, 0)).normalized()

    @property
    def up(self):
        return self.right.cross(self.forward).normalized()