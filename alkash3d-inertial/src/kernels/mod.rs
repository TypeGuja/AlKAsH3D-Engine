//! Fortran FFI bindings
//!
//! Связь между Rust и Fortran ядром физики

use std::ffi::c_void;

// Fortran типы (должны совпадать с определением в Fortran)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranRigidBody {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub is_static: i32,
    pub is_asleep: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
}

// Внешние Fortran функции
extern "C" {
    pub fn integrate_bodies(
        bodies: *mut FortranRigidBody,
        n: i32,
        dt: f32,
    );

    pub fn solve_contacts(
        bodies: *mut FortranRigidBody,
        contacts: *mut FortranContact,
        n_contacts: i32,
        iterations: i32,
    );

    pub fn broad_phase_sap(
        bodies: *const FortranRigidBody,
        n: i32,
        pairs: *mut i32,
        pair_count: *mut i32,
    );

    pub fn narrow_phase_gjk(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    pub fn update_aabb(
        bodies: *mut FortranRigidBody,
        n: i32,
        min_bounds: *mut [f32; 3],
        max_bounds: *mut [f32; 3],
    );
}

/// Обёртка для безопасного вызова Fortran
pub struct FortranPhysics {
    bodies: Vec<FortranRigidBody>,
    contacts: Vec<FortranContact>,
    aabb_min: Vec<[f32; 3]>,
    aabb_max: Vec<[f32; 3]>,
}

impl FortranPhysics {
    pub fn new(max_bodies: usize) -> Self {
        Self {
            bodies: Vec::with_capacity(max_bodies),
            contacts: Vec::with_capacity(max_bodies * 2),
            aabb_min: vec![[0.0; 3]; max_bodies],
            aabb_max: vec![[0.0; 3]; max_bodies],
        }
    }

    pub fn add_body(&mut self, body: FortranRigidBody) {
        self.bodies.push(body);
    }

    pub fn integrate(&mut self, dt: f32) {
        unsafe {
            integrate_bodies(
                self.bodies.as_mut_ptr(),
                self.bodies.len() as i32,
                dt,
            );
        }
    }

    pub fn solve_contacts(&mut self, iterations: i32) {
        unsafe {
            solve_contacts(
                self.bodies.as_mut_ptr(),
                self.contacts.as_mut_ptr(),
                self.contacts.len() as i32,
                iterations,
            );
        }
    }

    pub fn update_bounds(&mut self) {
        unsafe {
            update_aabb(
                self.bodies.as_mut_ptr(),
                self.bodies.len() as i32,
                self.aabb_min.as_mut_ptr(),
                self.aabb_max.as_mut_ptr(),
            );
        }
    }
}