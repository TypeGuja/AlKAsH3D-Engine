use crate::math::{Vec3, Quat, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode { Translate, Rotate, Scale, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoSpace { World, Local }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoAxis { X, Y, Z, XY, YZ, XZ, None }

pub struct Gizmo {
    pub mode: GizmoMode,
    pub space: GizmoSpace,
    pub position: Vec3,
    pub rotation: Quat,
    pub visible: bool,
    pub dragging: bool,
    pub active_axis: GizmoAxis,
    pub drag_start_pos: Vec3,
    pub drag_start_value: Vec3,
}

impl Default for Gizmo {
    fn default() -> Self {
        Self {
            mode: GizmoMode::Translate,
            space: GizmoSpace::World,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            visible: true,
            dragging: false,
            active_axis: GizmoAxis::None,
            drag_start_pos: Vec3::ZERO,
            drag_start_value: Vec3::ZERO,
        }
    }
}

impl Gizmo {
    pub fn update(&mut self, transform: Transform) {
        self.position = transform.position;
        self.rotation = transform.rotation;
    }

    pub fn begin_drag(&mut self, axis: GizmoAxis, start_pos: Vec3) {
        self.dragging = true;
        self.active_axis = axis;
        self.drag_start_pos = start_pos;
        self.drag_start_value = self.position;
    }

    pub fn drag(&mut self, delta: Vec3) -> Transform {
        let mut new_transform = Transform {
            position: self.position,
            rotation: self.rotation,
            scale: Vec3::ONE,
        };

        match (self.mode, self.active_axis) {
            (GizmoMode::Translate, GizmoAxis::X) => {
                new_transform.position.x = self.drag_start_value.x + delta.x;
            }
            (GizmoMode::Translate, GizmoAxis::Y) => {
                new_transform.position.y = self.drag_start_value.y + delta.y;
            }
            (GizmoMode::Translate, GizmoAxis::Z) => {
                new_transform.position.z = self.drag_start_value.z + delta.z;
            }
            (GizmoMode::Rotate, GizmoAxis::X) => {
                new_transform.rotation = self.rotation.mul(&Quat::from_axis_angle(Vec3::RIGHT, delta.x * 0.01));
            }
            (GizmoMode::Rotate, GizmoAxis::Y) => {
                new_transform.rotation = self.rotation.mul(&Quat::from_axis_angle(Vec3::UP, delta.x * 0.01));
            }
            (GizmoMode::Rotate, GizmoAxis::Z) => {
                new_transform.rotation = self.rotation.mul(&Quat::from_axis_angle(Vec3::FORWARD, delta.x * 0.01));
            }
            (GizmoMode::Scale, GizmoAxis::X) => {
                new_transform.scale.x = (self.drag_start_value.x + delta.x * 0.01).max(0.01);
            }
            (GizmoMode::Scale, GizmoAxis::Y) => {
                new_transform.scale.y = (self.drag_start_value.y + delta.y * 0.01).max(0.01);
            }
            (GizmoMode::Scale, GizmoAxis::Z) => {
                new_transform.scale.z = (self.drag_start_value.z + delta.z * 0.01).max(0.01);
            }
            _ => {}
        }

        new_transform
    }

    pub fn end_drag(&mut self) {
        self.dragging = false;
        self.active_axis = GizmoAxis::None;
    }
}