// src/body.rs
use nalgebra as na;
use crate::{Vector3, Matrix4, Transform};

#[derive(Debug, Clone)]
pub struct RigidBody {
    pub id: u32,
    pub position: Vector3,
    pub rotation: na::UnitQuaternion<f32>,
    pub velocity: Vector3,
    pub acceleration: Vector3,
    pub angular_velocity: Vector3,
    pub angular_acceleration: Vector3,
    pub mass: f32,
    pub inv_mass: f32,
    pub inertia_tensor: Matrix4,
    pub inv_inertia_tensor: Matrix4,
    pub force_accumulator: Vector3,
    pub torque_accumulator: Vector3,
    pub restitution: f32,
    pub friction: f32,
    pub is_static: bool,
    pub is_kinematic: bool,
    pub is_asleep: bool,
    pub sleep_timer: f32,
    pub linear_sleep_threshold: f32,
    pub angular_sleep_threshold: f32,
}

impl RigidBody {
    pub fn new(mass: f32, position: Vector3) -> Self {
        let inv_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };

        Self {
            id: 0,
            position,
            rotation: na::UnitQuaternion::identity(),
            velocity: Vector3::zeros(),
            acceleration: Vector3::zeros(),
            angular_velocity: Vector3::zeros(),
            angular_acceleration: Vector3::zeros(),
            mass,
            inv_mass,
            inertia_tensor: Matrix4::identity(),
            inv_inertia_tensor: Matrix4::identity(),
            force_accumulator: Vector3::zeros(),
            torque_accumulator: Vector3::zeros(),
            restitution: 0.5,
            friction: 0.5,
            is_static: mass <= 0.0,
            is_kinematic: false,
            is_asleep: false,
            sleep_timer: 0.0,
            linear_sleep_threshold: 0.01,
            angular_sleep_threshold: 0.01,
        }
    }

    pub fn apply_force(&mut self, force: Vector3, point: Vector3) {
        if self.is_static { return; }
        self.force_accumulator += force;
        self.torque_accumulator += (point - self.position).cross(&force);
    }

    pub fn apply_force_center(&mut self, force: Vector3) {
        if self.is_static { return; }
        self.force_accumulator += force;
    }

    pub fn apply_impulse(&mut self, impulse: Vector3, point: Vector3) {
        if self.is_static { return; }
        self.velocity += impulse * self.inv_mass;
        // Упрощённо: без тензора инерции для начала
        // self.angular_velocity += self.inv_inertia_tensor * (point - self.position).cross(&impulse);
    }

    pub fn get_transform(&self) -> Transform {
        Transform::new(self.position, self.rotation, Vector3::new(1.0, 1.0, 1.0))
    }

    pub fn can_sleep(&mut self, dt: f32) -> bool {
        if self.is_static || self.is_kinematic {
            return true;
        }

        let linear_speed = self.velocity.magnitude();
        let angular_speed = self.angular_velocity.magnitude();

        if linear_speed < self.linear_sleep_threshold &&
            angular_speed < self.angular_sleep_threshold {
            self.sleep_timer += dt;
            self.sleep_timer > 2.0
        } else {
            self.sleep_timer = 0.0;
            false
        }
    }

    pub fn wake_up(&mut self) {
        self.is_asleep = false;
        self.sleep_timer = 0.0;
    }
}