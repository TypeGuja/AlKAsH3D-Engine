//! 3D гизмо для трансформации объектов

use crate::math::{Vec3, Ray, Mat4, Quat, Plane, Transform};
use egui::Pos2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode {
    Translate,
    Rotate,
    Scale,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoAxis {
    X, Y, Z,
    XY, YZ, XZ, // Для плоскостей в режиме Translate
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoSpace {
    World,
    Local,
}

pub struct Gizmo {
    pub mode: GizmoMode,
    pub space: GizmoSpace,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub visible: bool,
    pub dragging: bool,
    pub active_axis: GizmoAxis,
    pub snap_enabled: bool,
    pub snap_translate: f32,
    pub snap_rotate: f32,
    pub snap_scale: f32,

    // Состояние перетаскивания
    drag_start_pos: Vec3,
    drag_start_mouse: Pos2,
    drag_start_value: Vec3,
    drag_plane: Plane,
}

impl Default for Gizmo {
    fn default() -> Self {
        Self {
            mode: GizmoMode::Translate,
            space: GizmoSpace::World,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            visible: true,
            dragging: false,
            active_axis: GizmoAxis::None,
            snap_enabled: false,
            snap_translate: 1.0,
            snap_rotate: 15.0,
            snap_scale: 0.1,
            drag_start_pos: Vec3::ZERO,
            drag_start_mouse: Pos2::ZERO,
            drag_start_value: Vec3::ZERO,
            drag_plane: Plane {
                normal: Vec3::Y,
                distance: 0.0,
            },
        }
    }
}

impl Gizmo {
    pub fn update(&mut self, transform: Transform) {
        self.position = transform.translation;
        self.rotation = transform.rotation;
        self.scale = transform.scale;
    }

    pub fn apply_transform(&self, transform: &mut Transform) {
        if !self.dragging {
            return;
        }

        match self.mode {
            GizmoMode::Translate => {
                transform.translation = self.position;
            }
            GizmoMode::Rotate => {
                transform.rotation = self.rotation;
            }
            GizmoMode::Scale => {
                transform.scale = self.scale;
            }
            GizmoMode::None => {}
        }
    }

    pub fn begin_drag(
        &mut self,
        mouse_pos: Pos2,
        ray: Ray,
        camera_pos: Vec3,
    ) -> bool {
        let gizmo_size = self.get_gizmo_size(camera_pos);

        // Проверяем пересечение с осями
        self.active_axis = self.intersect_axis(ray, gizmo_size);

        if self.active_axis != GizmoAxis::None {
            self.dragging = true;
            self.drag_start_mouse = mouse_pos;
            self.drag_start_pos = self.position;
            self.drag_start_value = match self.mode {
                GizmoMode::Translate => self.position,
                GizmoMode::Rotate => Vec3::ZERO,
                GizmoMode::Scale => self.scale,
                GizmoMode::None => Vec3::ZERO,
            };

            // Создаем плоскость для перетаскивания
            self.drag_plane = self.create_drag_plane(ray, camera_pos);

            return true;
        }

        false
    }

    pub fn drag(&mut self, mouse_pos: Pos2, ray: Ray) {
        if !self.dragging {
            return;
        }

        if let Some(hit_point) = ray.intersect_plane(self.position, self.drag_plane.normal) {
            let delta = hit_point - self.drag_start_pos;

            match (self.mode, self.active_axis) {
                (GizmoMode::Translate, axis) => {
                    let axis_vec = self.get_axis_vector(axis);
                    let projected_delta = axis_vec * axis_vec.dot(delta);

                    let mut new_pos = self.drag_start_value + projected_delta;

                    if self.snap_enabled {
                        new_pos = self.snap_vector(new_pos, self.snap_translate);
                    }

                    self.position = new_pos;
                }
                (GizmoMode::Rotate, axis) => {
                    let axis_vec = self.get_axis_vector(axis);
                    let delta_angle = (mouse_pos.x - self.drag_start_mouse.x) * 0.01;

                    let mut angle = delta_angle;
                    if self.snap_enabled {
                        angle = (angle / self.snap_rotate.to_radians()).round()
                            * self.snap_rotate.to_radians();
                    }

                    self.rotation = self.rotation
                        * Quat::from_axis_angle(axis_vec, angle);
                }
                (GizmoMode::Scale, axis) => {
                    let axis_vec = self.get_axis_vector(axis);
                    let projected_delta = axis_vec * axis_vec.dot(delta);
                    let scale_factor = 1.0 + projected_delta.length() * 0.01
                        * projected_delta.dot(axis_vec).signum();

                    let mut new_scale = self.drag_start_value;
                    match axis {
                        GizmoAxis::X => new_scale.x *= scale_factor,
                        GizmoAxis::Y => new_scale.y *= scale_factor,
                        GizmoAxis::Z => new_scale.z *= scale_factor,
                        GizmoAxis::XY | GizmoAxis::YZ | GizmoAxis::XZ => {
                            // Равномерное масштабирование по двум осям
                            let avg_factor = scale_factor;
                            match axis {
                                GizmoAxis::XY => {
                                    new_scale.x *= avg_factor;
                                    new_scale.y *= avg_factor;
                                }
                                GizmoAxis::YZ => {
                                    new_scale.y *= avg_factor;
                                    new_scale.z *= avg_factor;
                                }
                                GizmoAxis::XZ => {
                                    new_scale.x *= avg_factor;
                                    new_scale.z *= avg_factor;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }

                    if self.snap_enabled {
                        new_scale = self.snap_vector(new_scale, self.snap_scale);
                    }

                    self.scale = new_scale.max(Vec3::splat(0.01));
                }
                _ => {}
            }
        }
    }

    pub fn end_drag(&mut self) {
        self.dragging = false;
        self.active_axis = GizmoAxis::None;
    }

    fn get_gizmo_size(&self, camera_pos: Vec3) -> f32 {
        let distance = camera_pos.distance(self.position);
        distance * 0.15
    }

    fn intersect_axis(&self, ray: Ray, size: f32) -> GizmoAxis {
        let threshold = size * 0.1;

        // Проверяем оси
        let axes = [
            (GizmoAxis::X, Vec3::X),
            (GizmoAxis::Y, Vec3::Y),
            (GizmoAxis::Z, Vec3::Z),
        ];

        let mut closest_axis = GizmoAxis::None;
        let mut closest_dist = threshold;

        for (axis, dir) in axes {
            let axis_dir = match self.space {
                GizmoSpace::World => dir,
                GizmoSpace::Local => self.rotation * dir,
            };

            let axis_end = self.position + axis_dir * size;

            if let Some(t) = self.ray_cylinder_intersect(
                ray,
                self.position,
                axis_end,
                threshold,
            ) {
                if t < closest_dist {
                    closest_dist = t;
                    closest_axis = axis;
                }
            }
        }

        // Проверяем плоскости для Translate
        if self.mode == GizmoMode::Translate {
            let planes = [
                (GizmoAxis::XY, Vec3::Z),
                (GizmoAxis::YZ, Vec3::X),
                (GizmoAxis::XZ, Vec3::Y),
            ];

            for (plane, normal) in planes {
                let plane_normal = match self.space {
                    GizmoSpace::World => normal,
                    GizmoSpace::Local => self.rotation * normal,
                };

                let plane_center = self.position + plane_normal * size * 0.3;

                if let Some(hit) = ray.intersect_plane(plane_center, plane_normal) {
                    let local_hit = hit - plane_center;
                    if local_hit.length() < size * 0.4 {
                        let dist = hit.distance(ray.origin);
                        if dist < closest_dist {
                            closest_dist = dist;
                            closest_axis = plane;
                        }
                    }
                }
            }
        }

        closest_axis
    }

    fn ray_cylinder_intersect(
        &self,
        ray: Ray,
        start: Vec3,
        end: Vec3,
        radius: f32,
    ) -> Option<f32> {
        let axis = end - start;
        let axis_len = axis.length();
        let axis_dir = axis / axis_len;

        let ao = ray.origin - start;

        let a = ray.direction - axis_dir * ray.direction.dot(axis_dir);
        let b = ao - axis_dir * ao.dot(axis_dir);

        let a_len = a.length();
        if a_len < 0.0001 {
            return None;
        }

        let t = -a.dot(b) / (a_len * a_len);

        if t < 0.0 {
            return None;
        }

        let hit_point = ray.origin + ray.direction * t;
        let proj = (hit_point - start).dot(axis_dir);

        if proj < 0.0 || proj > axis_len {
            return None;
        }

        let dist = (hit_point - (start + axis_dir * proj)).length();

        if dist <= radius {
            Some(t)
        } else {
            None
        }
    }

    fn create_drag_plane(&self, ray: Ray, camera_pos: Vec3) -> Plane {
        let normal = match self.active_axis {
            GizmoAxis::X => {
                if self.mode == GizmoMode::Translate {
                    let cam_dir = (camera_pos - self.position).normalize();
                    if cam_dir.dot(Vec3::Y).abs() > 0.7 {
                        Vec3::Y
                    } else {
                        Vec3::Z
                    }
                } else {
                    Vec3::X
                }
            }
            GizmoAxis::Y => {
                if self.mode == GizmoMode::Translate {
                    let cam_dir = (camera_pos - self.position).normalize();
                    if cam_dir.dot(Vec3::X).abs() > 0.7 {
                        Vec3::X
                    } else {
                        Vec3::Z
                    }
                } else {
                    Vec3::Y
                }
            }
            GizmoAxis::Z => {
                if self.mode == GizmoMode::Translate {
                    let cam_dir = (camera_pos - self.position).normalize();
                    if cam_dir.dot(Vec3::X).abs() > 0.7 {
                        Vec3::X
                    } else {
                        Vec3::Y
                    }
                } else {
                    Vec3::Z
                }
            }
            _ => Vec3::Y,
        };

        Plane::from_point_normal(self.position, normal)
    }

    fn get_axis_vector(&self, axis: GizmoAxis) -> Vec3 {
        let dir = match axis {
            GizmoAxis::X => Vec3::X,
            GizmoAxis::Y => Vec3::Y,
            GizmoAxis::Z => Vec3::Z,
            _ => Vec3::ZERO,
        };

        match self.space {
            GizmoSpace::World => dir,
            GizmoSpace::Local => self.rotation * dir,
        }
    }

    fn snap_vector(&self, v: Vec3, snap: f32) -> Vec3 {
        Vec3::new(
            (v.x / snap).round() * snap,
            (v.y / snap).round() * snap,
            (v.z / snap).round() * snap,
        )
    }

    pub fn render_commands(&self) -> Vec<GizmoRenderCommand> {
        let mut commands = Vec::new();

        if !self.visible {
            return commands;
        }

        let size = 1.0; // Базовый размер

        // Оси
        let axes = [
            (GizmoAxis::X, Vec3::X, [1.0, 0.2, 0.2]), // Красный
            (GizmoAxis::Y, Vec3::Y, [0.2, 1.0, 0.2]), // Зеленый
            (GizmoAxis::Z, Vec3::Z, [0.2, 0.2, 1.0]), // Синий
        ];

        for (axis, dir, color) in axes {
            let axis_dir = match self.space {
                GizmoSpace::World => dir,
                GizmoSpace::Local => self.rotation * dir,
            };

            let is_active = self.active_axis == axis;
            let line_color = if is_active {
                [1.0, 1.0, 0.2] // Желтый при активации
            } else {
                color
            };

            let end = self.position + axis_dir * size;

            commands.push(GizmoRenderCommand::Line {
                start: self.position,
                end,
                color: line_color,
                thickness: if is_active { 3.0 } else { 2.0 },
            });

            // Стрелка
            let arrow_size = size * 0.15;
            let arrow_pos = end - axis_dir * arrow_size;
            commands.push(GizmoRenderCommand::Cone {
                position: arrow_pos,
                direction: axis_dir,
                height: arrow_size,
                radius: arrow_size * 0.5,
                color: line_color,
            });
        }

        // Плоскости для Translate
        if self.mode == GizmoMode::Translate {
            let planes = [
                (GizmoAxis::XY, Vec3::X, Vec3::Y, [0.2, 0.2, 1.0]),
                (GizmoAxis::YZ, Vec3::Y, Vec3::Z, [1.0, 0.2, 0.2]),
                (GizmoAxis::XZ, Vec3::X, Vec3::Z, [0.2, 1.0, 0.2]),
            ];

            for (plane_axis, dir1, dir2, color) in planes {
                let (d1, d2) = match self.space {
                    GizmoSpace::World => (dir1, dir2),
                    GizmoSpace::Local => (self.rotation * dir1, self.rotation * dir2),
                };

                let is_active = self.active_axis == plane_axis;
                let plane_color = if is_active {
                    [1.0, 1.0, 0.2, 0.5]
                } else {
                    [color[0], color[1], color[2], 0.3]
                };

                let center = self.position + (d1 + d2) * size * 0.3;

                commands.push(GizmoRenderCommand::Quad {
                    center,
                    u: d1 * size * 0.25,
                    v: d2 * size * 0.25,
                    color: plane_color,
                });
            }
        }

        // Центральная точка
        commands.push(GizmoRenderCommand::Sphere {
            center: self.position,
            radius: size * 0.05,
            color: [1.0, 1.0, 1.0],
        });

        commands
    }
}

#[derive(Debug, Clone)]
pub enum GizmoRenderCommand {
    Line {
        start: Vec3,
        end: Vec3,
        color: [f32; 3],
        thickness: f32,
    },
    Cone {
        position: Vec3,
        direction: Vec3,
        height: f32,
        radius: f32,
        color: [f32; 3],
    },
    Sphere {
        center: Vec3,
        radius: f32,
        color: [f32; 3],
    },
    Quad {
        center: Vec3,
        u: Vec3,
        v: Vec3,
        color: [f32; 4],
    },
}