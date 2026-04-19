//! Математический модуль с использованием glam

pub use glam::{Vec2, Vec3, Vec4, Mat4, Quat, EulerRot};
use serde::{Serialize, Deserialize};

// Реэкспортируем типы для удобства
pub type Vector2 = Vec2;
pub type Vector3 = Vec3;
pub type Vector4 = Vec4;
pub type Matrix4 = Mat4;
pub type Quaternion = Quat;

// ============================================================
// Transform
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform {
    pub const fn identity() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::identity()
        }
    }

    pub fn from_rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            ..Self::identity()
        }
    }

    pub fn from_scale(scale: Vec3) -> Self {
        Self {
            scale,
            ..Self::identity()
        }
    }

    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            self.scale,
            self.rotation,
            self.translation
        )
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    pub fn look_at(&mut self, target: Vec3, up: Vec3) {
        let direction = (target - self.translation).normalize();
        self.rotation = Quat::from_mat3(&glam::Mat3::from_cols(
            up.cross(direction).normalize(),
            direction.cross(up.cross(direction)).normalize(),
            direction,
        ));
    }

    pub fn rotate(&mut self, euler: Vec3) {
        self.rotation = Quat::from_euler(EulerRot::XYZ, euler.x, euler.y, euler.z) * self.rotation;
    }

    pub fn translate(&mut self, delta: Vec3) {
        self.translation += delta;
    }

    pub fn scale_by(&mut self, factor: Vec3) {
        self.scale *= factor;
    }
}

// ============================================================
// Camera
// ============================================================

#[derive(Debug, Clone)]
pub struct Camera {
    pub transform: Transform,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub orthographic: bool,
    pub ortho_size: f32,
}

impl Camera {
    pub fn new(aspect: f32) -> Self {
        let mut camera = Self {
            transform: Transform::from_translation(Vec3::new(5.0, 5.0, 10.0)),
            fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            aspect,
            orthographic: false,
            ortho_size: 10.0,
        };
        camera.look_at(Vec3::ZERO);
        camera
    }

    pub fn look_at(&mut self, target: Vec3) {
        let direction = (target - self.transform.translation).normalize();
        let right = Vec3::Y.cross(direction).normalize();
        let _up = direction.cross(right);
        self.transform.rotation = Quat::from_mat3(&glam::Mat3::from_cols(right, _up, direction));
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(
            self.transform.translation,
            self.transform.translation + self.transform.forward(),
            self.transform.up(),
        )
    }

    pub fn projection_matrix(&self) -> Mat4 {
        if self.orthographic {
            let half_h = self.ortho_size;
            let half_w = half_h * self.aspect;
            Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, self.near, self.far)
        } else {
            Mat4::perspective_rh(self.fov, self.aspect, self.near, self.far)
        }
    }

    pub fn view_projection_matrix(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }

    pub fn orbit(&mut self, delta: Vec2, target: Vec3) {
        let radius = self.transform.translation.distance(target);
        let direction = (self.transform.translation - target).normalize();

        let right = direction.cross(Vec3::Y).normalize();
        let up = right.cross(direction);

        let rot_h = Quat::from_axis_angle(Vec3::Y, -delta.x * 0.01);
        let rot_v = Quat::from_axis_angle(right, -delta.y * 0.01);

        let new_direction = rot_v * (rot_h * direction);

        self.transform.translation = target + new_direction * radius;
        self.look_at(target);
    }

    pub fn pan(&mut self, delta: Vec2) {
        let right = self.transform.right();
        let up = self.transform.up();
        let speed = self.transform.translation.length() * 0.001;

        self.transform.translation -= right * delta.x * speed;
        self.transform.translation += up * delta.y * speed;
    }

    pub fn zoom(&mut self, delta: f32) {
        if self.orthographic {
            self.ortho_size = (self.ortho_size - delta * 0.5).max(0.1);
        } else {
            let forward = self.transform.forward();
            let speed = self.transform.translation.length() * 0.1;
            self.transform.translation += forward * delta * speed;
        }
    }

    pub fn focus_on(&mut self, target: Vec3) {
        let direction = (self.transform.translation - target).normalize();
        let distance = self.transform.translation.distance(target);
        self.transform.translation = target + direction * distance;
        self.look_at(target);
    }
}

// ============================================================
// Ray
// ============================================================

#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn from_screen(
        screen_pos: Vec2,
        screen_size: Vec2,
        camera: &Camera,
    ) -> Self {
        let ndc = Vec2::new(
            2.0 * screen_pos.x / screen_size.x - 1.0,
            1.0 - 2.0 * screen_pos.y / screen_size.y,
        );

        let inv_proj = camera.projection_matrix().inverse();
        let inv_view = camera.view_matrix().inverse();

        let near_point_ndc = ndc.extend(0.0).extend(1.0);
        let far_point_ndc = ndc.extend(1.0).extend(1.0);

        let near_point_world = inv_view * inv_proj * near_point_ndc;
        let far_point_world = inv_view * inv_proj * far_point_ndc;

        let near_point = Vec3::new(
            near_point_world.x / near_point_world.w,
            near_point_world.y / near_point_world.w,
            near_point_world.z / near_point_world.w,
        );
        let far_point = Vec3::new(
            far_point_world.x / far_point_world.w,
            far_point_world.y / far_point_world.w,
            far_point_world.z / far_point_world.w,
        );

        let direction = (far_point - near_point).normalize();

        Self {
            origin: near_point,
            direction,
        }
    }

    pub fn intersect_plane(&self, plane_point: Vec3, plane_normal: Vec3) -> Option<Vec3> {
        let denom = plane_normal.dot(self.direction);
        if denom.abs() < 0.0001 {
            return None;
        }

        let t = (plane_point - self.origin).dot(plane_normal) / denom;
        if t < 0.0 {
            return None;
        }

        Some(self.origin + self.direction * t)
    }

    pub fn intersect_sphere(&self, center: Vec3, radius: f32) -> Option<f32> {
        let oc = self.origin - center;
        let a = self.direction.dot(self.direction);
        let b = 2.0 * oc.dot(self.direction);
        let c = oc.dot(oc) - radius * radius;
        let discriminant = b * b - 4.0 * a * c;

        if discriminant < 0.0 {
            return None;
        }

        let t = (-b - discriminant.sqrt()) / (2.0 * a);
        if t >= 0.0 {
            Some(t)
        } else {
            None
        }
    }

    pub fn intersect_aabb(&self, min: Vec3, max: Vec3) -> Option<f32> {
        let inv_dir = Vec3::new(
            1.0 / self.direction.x,
            1.0 / self.direction.y,
            1.0 / self.direction.z,
        );

        let t1 = (min - self.origin) * inv_dir;
        let t2 = (max - self.origin) * inv_dir;

        let tmin = t1.min(t2);
        let tmax = t1.max(t2);

        let t_near = tmin.x.max(tmin.y).max(tmin.z);
        let t_far = tmax.x.min(tmax.y).min(tmax.z);

        if t_near <= t_far && t_far >= 0.0 {
            Some(t_near.max(0.0))
        } else {
            None
        }
    }
}

// ============================================================
// Bounding Box
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AABB {
    pub min: Vec3,
    pub max: Vec3,
}

impl AABB {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_points(points: &[Vec3]) -> Self {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);

        for point in points {
            min = min.min(*point);
            max = max.max(*point);
        }

        Self { min, max }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn radius(&self) -> f32 {
        self.size().length() * 0.5
    }

    pub fn transform(&self, transform: &Transform) -> Self {
        let corners = [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
        ];

        let transformed: Vec<Vec3> = corners.iter()
            .map(|c| transform.to_matrix().transform_point3(*c))
            .collect();

        Self::from_points(&transformed)
    }
}

impl Default for AABB {
    fn default() -> Self {
        Self {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
        }
    }
}

// ============================================================
// Plane
// ============================================================

#[derive(Debug, Clone, Copy)]
pub struct Plane {
    pub normal: Vec3,
    pub distance: f32,
}

impl Plane {
    pub fn from_point_normal(point: Vec3, normal: Vec3) -> Self {
        let normal = normal.normalize();
        Self {
            normal,
            distance: -normal.dot(point),
        }
    }

    pub fn distance_to_point(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.distance
    }
}