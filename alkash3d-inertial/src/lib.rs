// inertial/src/lib.rs
//! Inertial Physics Engine - оптимизированная версия с поддержкой параллельных вычислений

use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use rayon::prelude::*;

// ===================================================================
// FFI структуры (совместимые с движком)
// ===================================================================

#[repr(C)]
pub struct PhysicsConfig {
    pub max_bodies: i32,
    pub world_size: f32,
    pub cell_size: f32,
    pub solver_iterations: i32,
    pub use_simd: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsBody {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angular_acceleration: [f32; 3],
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub is_static: i32,
    pub is_asleep: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicsStats {
    pub bodies_count: u32,
    pub active_bodies: u32,
    pub contacts_count: u32,
    pub pairs_count: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
}

// ===================================================================
// Physics API
// ===================================================================

#[repr(C)]
pub struct PhysicsAPI {
    pub add_body: extern "C" fn(instance: *mut c_void, body: *const PhysicsBody) -> i32,
    pub remove_body: extern "C" fn(instance: *mut c_void, id: i32),
    pub get_body: extern "C" fn(instance: *mut c_void, id: i32) -> PhysicsBody,
    pub get_bodies_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub update: extern "C" fn(instance: *mut c_void, dt: f32, gravity: f32, num_threads: u32),
    pub get_contacts: extern "C" fn(instance: *mut c_void) -> *const PhysicsContact,
    pub get_contacts_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_pairs: extern "C" fn(instance: *mut c_void) -> *const i32,
    pub get_pairs_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_stats: extern "C" fn(instance: *mut c_void) -> PhysicsStats,
}

// ===================================================================
// Plugin API (для совместимости с движком)
// ===================================================================

pub const PLUGIN_API_VERSION: u32 = 1;
pub const PLUGIN_TYPE_PHYSICS: u32 = 0;

#[repr(C)]
pub struct PluginAPI {
    pub version: u32,
    pub plugin_type: u32,
    pub name: *const std::os::raw::c_char,
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

// ===================================================================
// Внутреннее состояние (с Mutex для потокобезопасности)
// ===================================================================

struct PhysicsState {
    bodies: Mutex<Vec<PhysicsBody>>,
    contacts: Mutex<Vec<PhysicsContact>>,
    pairs: Mutex<Vec<i32>>,
    stats: Mutex<PhysicsStats>,
    num_threads: u32,
}

impl PhysicsState {
    fn new(config: &PhysicsConfig) -> Self {
        Self {
            bodies: Mutex::new(Vec::with_capacity(config.max_bodies as usize)),
            contacts: Mutex::new(Vec::new()),
            pairs: Mutex::new(Vec::new()),
            stats: Mutex::new(PhysicsStats::default()),
            num_threads: 4,
        }
    }

    fn add_body(&self, body: &PhysicsBody) -> i32 {
        let mut bodies = self.bodies.lock().unwrap();
        let id = bodies.len() as i32;
        bodies.push(*body);
        let mut stats = self.stats.lock().unwrap();
        stats.bodies_count = bodies.len() as u32;
        id
    }

    fn update(&self, dt: f32, gravity: f32) {
        let radius = 0.5;
        let radius_sum = radius + radius;
        let radius_sum_sq = radius_sum * radius_sum;

        // Захватываем тела для чтения и записи
        let mut bodies = self.bodies.lock().unwrap();
        let bodies_len = bodies.len();

        if bodies_len == 0 {
            return;
        }

        // === ОБНОВЛЕНИЕ ПОЗИЦИЙ ===
        for body in bodies.iter_mut() {
            if body.is_asleep == 0 && body.is_static == 0 {
                body.velocity[1] += gravity * dt;
                body.position[0] += body.velocity[0] * dt;
                body.position[1] += body.velocity[1] * dt;
                body.position[2] += body.velocity[2] * dt;
                body.velocity[0] *= 1.0 - body.linear_damping * dt;
                body.velocity[1] *= 1.0 - body.linear_damping * dt;
                body.velocity[2] *= 1.0 - body.linear_damping * dt;
            }
        }

        // === ПОИСК ПАР КОЛЛИЗИЙ ===
        let mut pairs: Vec<(usize, usize)> = Vec::new();

        for i in 0..bodies_len {
            let a = &bodies[i];
            if a.is_asleep == 1 { continue; }

            for j in (i + 1)..bodies_len {
                let b = &bodies[j];
                if b.is_asleep == 1 { continue; }

                let dx = b.position[0] - a.position[0];
                let dy = b.position[1] - a.position[1];
                let dz = b.position[2] - a.position[2];
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq < radius_sum_sq {
                    pairs.push((i, j));
                }
            }
        }

        // === ГЕНЕРАЦИЯ КОНТАКТОВ ===
        let mut contacts = Vec::with_capacity(pairs.len());
        let mut pairs_vec = Vec::with_capacity(pairs.len() * 2);

        for (i, j) in pairs {
            let a = &bodies[i];
            let b = &bodies[j];

            let dx = b.position[0] - a.position[0];
            let dy = b.position[1] - a.position[1];
            let dz = b.position[2] - a.position[2];
            let dist_sq = dx * dx + dy * dy + dz * dz;
            let distance = if dist_sq > 0.0 { dist_sq.sqrt() } else { 0.001 };
            let inv_dist = 1.0 / distance;
            let nx = dx * inv_dist;
            let ny = dy * inv_dist;
            let nz = dz * inv_dist;
            let penetration = radius_sum - distance;

            contacts.push(PhysicsContact {
                body_a: i as i32,
                body_b: j as i32,
                normal: [nx, ny, nz],
                penetration,
                point: [
                    a.position[0] + nx * radius,
                    a.position[1] + ny * radius,
                    a.position[2] + nz * radius,
                ],
            });

            pairs_vec.push(i as i32);
            pairs_vec.push(j as i32);
        }

        // === РАЗРЕШЕНИЕ КОЛЛИЗИЙ ===
        for contact in &contacts {
            let i = contact.body_a as usize;
            let j = contact.body_b as usize;

            if i >= bodies_len || j >= bodies_len { continue; }

            // Временные заимствования с split_at_mut для избежания конфликтов
            if i < j {
                let (left, right) = bodies.split_at_mut(j);
                let body_i = &mut left[i];
                let body_j = &mut right[0];
                Self::resolve_contact(body_i, body_j, contact);
            } else {
                let (left, right) = bodies.split_at_mut(i);
                let body_j = &mut left[j];
                let body_i = &mut right[0];
                Self::resolve_contact(body_i, body_j, contact);
            }
        }

        // Обновляем статистику
        let mut stats = self.stats.lock().unwrap();
        stats.contacts_count = contacts.len() as u32;
        stats.pairs_count = (pairs_vec.len() / 2) as u32;

        // Сохраняем контакты и пары
        let mut contacts_storage = self.contacts.lock().unwrap();
        *contacts_storage = contacts;
        let mut pairs_storage = self.pairs.lock().unwrap();
        *pairs_storage = pairs_vec;
    }

    fn resolve_contact(body_i: &mut PhysicsBody, body_j: &mut PhysicsBody, contact: &PhysicsContact) {
        let restitution = (body_i.restitution + body_j.restitution) * 0.5;
        let rel_vx = body_j.velocity[0] - body_i.velocity[0];
        let rel_vy = body_j.velocity[1] - body_i.velocity[1];
        let rel_vz = body_j.velocity[2] - body_i.velocity[2];
        let vel_along = rel_vx * contact.normal[0] + rel_vy * contact.normal[1] + rel_vz * contact.normal[2];

        if vel_along < 0.0 {
            let impulse_val = -(1.0 + restitution) * vel_along;
            let inv_mass_sum = body_i.inv_mass + body_j.inv_mass;

            if inv_mass_sum > 0.0 {
                let j_val = impulse_val / inv_mass_sum;
                let jx = contact.normal[0] * j_val;
                let jy = contact.normal[1] * j_val;
                let jz = contact.normal[2] * j_val;

                if body_i.is_static == 0 {
                    body_i.velocity[0] -= jx * body_i.inv_mass;
                    body_i.velocity[1] -= jy * body_i.inv_mass;
                    body_i.velocity[2] -= jz * body_i.inv_mass;
                }
                if body_j.is_static == 0 {
                    body_j.velocity[0] += jx * body_j.inv_mass;
                    body_j.velocity[1] += jy * body_j.inv_mass;
                    body_j.velocity[2] += jz * body_j.inv_mass;
                }
            }
        }

        let correction = contact.normal[0] * contact.penetration * 0.2;
        if body_i.is_static == 0 {
            body_i.position[0] -= correction;
            body_i.position[1] -= correction;
            body_i.position[2] -= correction;
        }
        if body_j.is_static == 0 {
            body_j.position[0] += correction;
            body_j.position[1] += correction;
            body_j.position[2] += correction;
        }
    }
}

// ===================================================================
// Экспортируемые функции
// ===================================================================

extern "C" fn add_body(instance: *mut c_void, body: *const PhysicsBody) -> i32 {
    if instance.is_null() || body.is_null() { return -1; }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        state.add_body(&*body)
    }
}

extern "C" fn remove_body(instance: *mut c_void, id: i32) {
    if instance.is_null() { return; }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let mut bodies = state.bodies.lock().unwrap();
        if id >= 0 && (id as usize) < bodies.len() {
            bodies.remove(id as usize);
        }
    }
}

extern "C" fn get_body(instance: *mut c_void, id: i32) -> PhysicsBody {
    if instance.is_null() {
        return unsafe { std::mem::zeroed() };
    }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let bodies = state.bodies.lock().unwrap();
        if id >= 0 && (id as usize) < bodies.len() {
            bodies[id as usize]
        } else {
            std::mem::zeroed()
        }
    }
}

extern "C" fn get_bodies_count(instance: *mut c_void) -> i32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let bodies = state.bodies.lock().unwrap();
        bodies.len() as i32
    }
}

extern "C" fn update_physics(instance: *mut c_void, dt: f32, gravity: f32, num_threads: u32) {
    if instance.is_null() { return; }
    unsafe {
        let state = &*(instance as *const PhysicsState);

        // Отладка: выводим позицию первого тела
        let bodies = state.bodies.lock().unwrap();
        if bodies.len() > 0 {
            let body = &bodies[0];
            println!("[Physics] Body 0 pos: ({:.2}, {:.2}, {:.2}), vel: ({:.2}, {:.2}, {:.2})",
                     body.position[0], body.position[1], body.position[2],
                     body.velocity[0], body.velocity[1], body.velocity[2]);
        }
        drop(bodies);

        state.update(dt, gravity);
    }
}

extern "C" fn get_contacts(instance: *mut c_void) -> *const PhysicsContact {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let contacts = state.contacts.lock().unwrap();
        if contacts.is_empty() {
            ptr::null()
        } else {
            contacts.as_ptr()
        }
    }
}

extern "C" fn get_contacts_count(instance: *mut c_void) -> i32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let contacts = state.contacts.lock().unwrap();
        contacts.len() as i32
    }
}

extern "C" fn get_pairs(instance: *mut c_void) -> *const i32 {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let pairs = state.pairs.lock().unwrap();
        if pairs.is_empty() {
            ptr::null()
        } else {
            pairs.as_ptr()
        }
    }
}

extern "C" fn get_pairs_count(instance: *mut c_void) -> i32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let pairs = state.pairs.lock().unwrap();
        (pairs.len() / 2) as i32
    }
}

extern "C" fn get_stats(instance: *mut c_void) -> PhysicsStats {
    if instance.is_null() { return PhysicsStats::default(); }
    unsafe {
        let state = &*(instance as *const PhysicsState);
        let stats = state.stats.lock().unwrap();
        *stats
    }
}

static PHYSICS_API: PhysicsAPI = PhysicsAPI {
    add_body,
    remove_body,
    get_body,
    get_bodies_count,
    update: update_physics,
    get_contacts,
    get_contacts_count,
    get_pairs,
    get_pairs_count,
    get_stats,
};

extern "C" fn init_physics(_device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void {
    if config_ptr.is_null() { return ptr::null_mut(); }
    unsafe {
        let config = &*(config_ptr as *const PhysicsConfig);
        let state = Box::new(PhysicsState::new(config));
        Box::into_raw(state) as *mut c_void
    }
}

extern "C" fn shutdown_physics(instance: *mut c_void) {
    if !instance.is_null() {
        unsafe { let _ = Box::from_raw(instance as *mut PhysicsState); }
    }
}

extern "C" fn update_plugin(_instance: *mut c_void, _dt: f32) {}

extern "C" fn get_physics_api(_instance: *mut c_void) -> *const c_void {
    &PHYSICS_API as *const _ as *const c_void
}

extern "C" fn get_light_api(_instance: *mut c_void) -> *const c_void {
    ptr::null()
}

// ===================================================================
// Главная экспортируемая функция
// ===================================================================

#[no_mangle]

pub extern "C" fn get_plugin_api() -> PluginAPI {
    PluginAPI {
        version: PLUGIN_API_VERSION,
        plugin_type: PLUGIN_TYPE_PHYSICS,
        name: b"Inertial Physics\0".as_ptr() as *const std::os::raw::c_char,
        init: init_physics,
        shutdown: shutdown_physics,
        update: update_plugin,
        get_physics_api,
        get_light_api,
    }
}