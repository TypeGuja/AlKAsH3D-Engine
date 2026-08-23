// inertial/src/lib.rs
//! Физический плагин "inertial" для Alkash3D — ABI-совместимая обёртка
//! (PluginAPI/PhysicsAPI) поверх РЕАЛЬНОГО Fortran-солвера.
//!
//! ИСПРАВЛЕНО (полная история): раньше этот файл содержал полностью
//! самостоятельную, наивную O(N²) Rust-реализацию физики — не
//! использующую ни Fortran-ядра (broad_phase.f90/narrow_phase.f90/
//! solver.f90/kernels_optimized.f90), ни объявленный, но неиспользуемый
//! rayon. В ней же был реальный баг в `resolve_contact`: коррекция
//! проникновения брала только normal[0] (X-компоненту нормали) и
//! применяла её ко всем трём осям позиции — то есть тела расталкивались
//! не вдоль нормали контакта, а в произвольном направлении. Плюс
//! `remove_body` использовал `Vec::remove` — сдвигая ID всех тел после
//! удалённого, из-за чего сохранённые где-то ID тихо начинали указывать
//! не на то тело. Плюс `println!` на каждый вызов `update_physics`.
//!
//! Теперь: вся физика реально считается в Fortran (broad phase — uniform
//! grid O(N), narrow phase — честный sphere-sphere тест, солвер —
//! безопасно распараллеленный через atomic-update, интеграция — по-
//! настоящему многопоточная через std::thread::scope), а этот файл —
//! только мост между ABI-структурами движка (PhysicsBody и т.п., без
//! информации о форме/инерции) и Fortran-структурами (FortranRigidBody, с
//! тензором инерции). ID тел — стабильные handle'ы, не совпадающие с
//! текущей позицией в солвере, так что удаление тел больше не портит
//! чужие ID.

mod ffi;

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::sync::Mutex;

use ffi::{FortranContact, FortranRigidBody, FortranPhysics};

// =====================================================================
// ABI СТРУКТУРЫ
//
// Определены здесь ЛОКАЛЬНО (эта DLL не зависит от крейта движка
// alkash3d_rs как от Cargo-зависимости) — контракт между движком и
// плагином это не общий Rust-тип, а совпадение #[repr(C)] layout'а.
// Должны побайтово совпадать с abi.rs/physics_api.rs движка.
// =====================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicsAPI {
    pub add_body: extern "C" fn(instance: *mut c_void, body: *const PhysicsBody) -> i32,
    pub remove_body: extern "C" fn(instance: *mut c_void, id: i32),
    pub get_body: extern "C" fn(instance: *mut c_void, id: i32) -> PhysicsBody,
    pub get_bodies_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub update: extern "C" fn(instance: *mut c_void, dt: f32, gravity: f32),
    pub get_contacts: extern "C" fn(instance: *mut c_void) -> *const PhysicsContact,
    pub get_contacts_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_pairs: extern "C" fn(instance: *mut c_void) -> *const i32,
    pub get_pairs_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_stats: extern "C" fn(instance: *mut c_void) -> PhysicsStats,
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum PluginType {
    Physics = 0,
    LightCulling = 1,
    Audio = 2,
    Scripting = 3,
}

#[repr(C)]
pub struct PluginAPI {
    pub version: u32,
    pub plugin_type: PluginType,
    pub name: *const c_char,
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

pub const PLUGIN_API_VERSION: u32 = 1;
static PLUGIN_NAME: &[u8] = b"inertial\0";

// =====================================================================
// Мостик PhysicsBody(ABI, без формы) <-> FortranRigidBody(с тензором инерции)
// =====================================================================

const IMPLICIT_RADIUS: f32 = 0.5;

fn to_fortran_body(b: &PhysicsBody) -> FortranRigidBody {
    let inertia_scalar = if b.mass > 0.0 {
        0.4 * b.mass * IMPLICIT_RADIUS * IMPLICIT_RADIUS
    } else {
        0.0
    };
    let inv_inertia_scalar = if inertia_scalar > 0.0 { 1.0 / inertia_scalar } else { 0.0 };
    let inertia = [
        [inertia_scalar, 0.0, 0.0],
        [0.0, inertia_scalar, 0.0],
        [0.0, 0.0, inertia_scalar],
    ];
    let inv_inertia = [
        [inv_inertia_scalar, 0.0, 0.0],
        [0.0, inv_inertia_scalar, 0.0],
        [0.0, 0.0, inv_inertia_scalar],
    ];

    FortranRigidBody {
        position: b.position,
        velocity: b.velocity,
        acceleration: b.acceleration,
        angular_velocity: b.angular_velocity,
        angular_acceleration: b.angular_acceleration,
        inertia,
        inv_inertia,
        mass: b.mass,
        inv_mass: b.inv_mass,
        restitution: b.restitution,
        friction: b.friction,
        linear_damping: b.linear_damping,
        angular_damping: b.angular_damping,
        is_static: b.is_static,
        is_asleep: b.is_asleep,
    }
}

fn to_abi_body(f: &FortranRigidBody) -> PhysicsBody {
    PhysicsBody {
        position: f.position,
        velocity: f.velocity,
        acceleration: f.acceleration,
        angular_velocity: f.angular_velocity,
        angular_acceleration: f.angular_acceleration,
        mass: f.mass,
        inv_mass: f.inv_mass,
        restitution: f.restitution,
        friction: f.friction,
        linear_damping: f.linear_damping,
        angular_damping: f.angular_damping,
        is_static: f.is_static,
        is_asleep: f.is_asleep,
    }
}

fn default_abi_body() -> PhysicsBody {
    PhysicsBody {
        position: [0.0; 3],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass: 0.0,
        inv_mass: 0.0,
        restitution: 0.0,
        friction: 0.0,
        linear_damping: 0.0,
        angular_damping: 0.0,
        is_static: 1,
        is_asleep: 1,
    }
}

// =====================================================================
// PhysicsState — реальная логика поверх Fortran-солвера
// =====================================================================

pub struct PhysicsState {
    config: PhysicsConfig,
    solver: FortranPhysics,
    next_handle: i32,
    handle_to_index: HashMap<i32, usize>,
    index_to_handle: Vec<i32>,
    contacts_abi: Vec<PhysicsContact>,
    pairs_abi: Vec<i32>,
    stats: PhysicsStats,
}

impl PhysicsState {
    fn new(config: PhysicsConfig) -> Self {
        let max_bodies = config.max_bodies.max(1) as usize;
        let world_size = if config.world_size > 0.0 { config.world_size } else { 100.0 };
        let cell_size = if config.cell_size > 0.0 { config.cell_size } else { 4.0 };

        Self {
            config,
            solver: FortranPhysics::new(max_bodies, world_size, cell_size),
            next_handle: 0,
            handle_to_index: HashMap::with_capacity(max_bodies),
            index_to_handle: Vec::with_capacity(max_bodies),
            contacts_abi: Vec::new(),
            pairs_abi: Vec::new(),
            stats: PhysicsStats::default(),
        }
    }

    fn add_body(&mut self, body: &PhysicsBody) -> i32 {
        let handle = self.next_handle;
        self.next_handle += 1;

        let idx = self.solver.bodies.len();
        self.solver.add_body(to_fortran_body(body));
        self.index_to_handle.push(handle);
        self.handle_to_index.insert(handle, idx);
        handle
    }

    fn remove_body(&mut self, handle: i32) {
        let Some(idx) = self.handle_to_index.remove(&handle) else {
            return;
        };
        if self.solver.bodies.is_empty() {
            return;
        }
        let last = self.solver.bodies.len() - 1;

        self.solver.bodies.swap_remove(idx);
        self.solver.sleep_timers.swap_remove(idx);

        if idx != last {
            let moved_handle = self.index_to_handle[last];
            self.index_to_handle[idx] = moved_handle;
            self.handle_to_index.insert(moved_handle, idx);
        }
        self.index_to_handle.pop();
    }

    fn get_body(&self, handle: i32) -> Option<PhysicsBody> {
        let &idx = self.handle_to_index.get(&handle)?;
        Some(to_abi_body(&self.solver.bodies[idx]))
    }

    fn bodies_count(&self) -> i32 {
        self.solver.bodies.len() as i32
    }

    fn update(&mut self, dt: f32, gravity: f32) {
        if self.solver.bodies.is_empty() {
            self.stats = PhysicsStats::default();
            self.contacts_abi.clear();
            self.pairs_abi.clear();
            return;
        }

        let t0 = std::time::Instant::now();
        let raw_pairs: Vec<i32> = self.solver.find_pairs_grid().to_vec();
        let broad_phase_time_ms = t0.elapsed().as_secs_f32() * 1000.0;

        let t1 = std::time::Instant::now();
        self.solver.clear_contacts();
        let mut internal_contacts: Vec<(usize, usize, FortranContact)> = Vec::new();
        for pair in raw_pairs.chunks_exact(2) {
            let ia = pair[0] as usize;
            let ib = pair[1] as usize;
            if ia >= self.solver.bodies.len() || ib >= self.solver.bodies.len() {
                continue;
            }
            // ИСПРАВЛЕНО (найдено по жалобе пользователя на просадки FPS
            // ПОСЛЕ того, как физика реально заработала): плотный опорный
            // "пол" из статических сфер (main.rs — 116 штук с нахлёстом,
            // чтобы не было щелей) взаимно ПЕРЕКРЫВАЕТСЯ соседями по
            // построению — каждая соседняя пара статичных опор физически
            // касается или проникает друг в друга. Broad phase честно
            // находит эти пары каждый кадр (они и правда близко), и без
            // этой проверки для КАЖДОЙ такой пары выполнялся полный GJK
            // (narrow_phase_gjk) и создавался контакт для solve_contacts —
            // то есть десятки-сотни contact-пар статика-статика
            // обрабатывались впустую каждый кадр: solve_contacts всё равно
            // ничего не может сделать с парой из двух тел с inv_mass=0
            // (resolve_contact_simple делит коррекцию по inv_mass, у обеих
            // total_inv_mass=0 — коррекция нулевая), но CPU на broad-phase-
            // совпадение + сам GJK-вызов + добавление в буфер контактов
            // тратится независимо от результата. Статика с статикой
            // физически никогда не должна порождать контакт для солвера —
            // пропускаем пару ДО дорогого narrow phase, а не после.
            if self.solver.bodies[ia].is_static != 0 && self.solver.bodies[ib].is_static != 0 {
                continue;
            }
            let mut contact = FortranContact::default();
            let hit = unsafe {
                ffi::narrow_phase_gjk(&self.solver.bodies[ia], &self.solver.bodies[ib], &mut contact)
            };
            if hit != 0 {
                contact.body_a = ia as i32;
                contact.body_b = ib as i32;
                internal_contacts.push((ia, ib, contact));
                self.solver.add_contact(contact);
            }
        }
        let narrow_phase_time_ms = t1.elapsed().as_secs_f32() * 1000.0;

        let t2 = std::time::Instant::now();
        if !self.solver.contacts.is_empty() {
            self.solver.solve_contacts_vectorized(self.config.solver_iterations.max(1), dt);
        }

        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(8);
        self.solver.batch_integrate(dt, gravity, num_threads);

        self.solver.update_sleep_state(dt, 0.01, 0.5);
        let solver_time_ms = t2.elapsed().as_secs_f32() * 1000.0;

        self.contacts_abi.clear();
        for (ia, ib, c) in &internal_contacts {
            self.contacts_abi.push(PhysicsContact {
                body_a: self.index_to_handle.get(*ia).copied().unwrap_or(-1),
                body_b: self.index_to_handle.get(*ib).copied().unwrap_or(-1),
                normal: c.normal,
                penetration: c.penetration,
                point: c.point,
            });
        }

        self.pairs_abi.clear();
        for pair in raw_pairs.chunks_exact(2) {
            let ia = pair[0] as usize;
            let ib = pair[1] as usize;
            self.pairs_abi.push(self.index_to_handle.get(ia).copied().unwrap_or(-1));
            self.pairs_abi.push(self.index_to_handle.get(ib).copied().unwrap_or(-1));
        }

        let active = self
            .solver
            .bodies
            .iter()
            .filter(|b| b.is_asleep == 0 && b.is_static == 0)
            .count() as u32;

        self.stats = PhysicsStats {
            bodies_count: self.solver.bodies.len() as u32,
            active_bodies: active,
            contacts_count: self.contacts_abi.len() as u32,
            pairs_count: (raw_pairs.len() / 2) as u32,
            broad_phase_time_ms,
            narrow_phase_time_ms,
            solver_time_ms,
        };
    }
}

// =====================================================================
// C ABI — экспортируемые функции плагина
// =====================================================================

struct PhysicsInstance {
    state: Mutex<PhysicsState>,
}

extern "C" fn physics_init(_device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void {
    let config = if config_ptr.is_null() {
        eprintln!("[INERTIAL] WARNING: init called with null config_ptr, using defaults");
        PhysicsConfig {
            max_bodies: 1000,
            world_size: 100.0,
            cell_size: 4.0,
            solver_iterations: 8,
            use_simd: 1,
        }
    } else {
        unsafe { *(config_ptr as *const PhysicsConfig) }
    };

    let instance = Box::new(PhysicsInstance {
        state: Mutex::new(PhysicsState::new(config)),
    });
    Box::into_raw(instance) as *mut c_void
}

extern "C" fn physics_shutdown(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(instance as *mut PhysicsInstance));
    }
}

extern "C" fn physics_plugin_update(instance: *mut c_void, dt: f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.update(dt, -9.81);
    }
}

extern "C" fn physics_get_physics_api(_instance: *mut c_void) -> *const c_void {
    &PHYSICS_API as *const PhysicsAPI as *const c_void
}

extern "C" fn physics_get_light_api(_instance: *mut c_void) -> *const c_void {
    std::ptr::null()
}

extern "C" fn api_add_body(instance: *mut c_void, body: *const PhysicsBody) -> i32 {
    if instance.is_null() || body.is_null() {
        return -1;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let body = unsafe { &*body };
    match inst.state.lock() {
        Ok(mut state) => state.add_body(body),
        Err(_) => -1,
    }
}

extern "C" fn api_remove_body(instance: *mut c_void, id: i32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.remove_body(id);
    }
}

extern "C" fn api_get_body(instance: *mut c_void, id: i32) -> PhysicsBody {
    if instance.is_null() {
        return default_abi_body();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.get_body(id).unwrap_or_else(default_abi_body),
        Err(_) => default_abi_body(),
    }
}

extern "C" fn api_get_bodies_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.bodies_count()).unwrap_or(0)
}

extern "C" fn api_update(instance: *mut c_void, dt: f32, gravity: f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.update(dt, gravity);
    }
}

extern "C" fn api_get_contacts(instance: *mut c_void) -> *const PhysicsContact {
    if instance.is_null() {
        return std::ptr::null();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.contacts_abi.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

extern "C" fn api_get_contacts_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.contacts_abi.len() as i32).unwrap_or(0)
}

extern "C" fn api_get_pairs(instance: *mut c_void) -> *const i32 {
    if instance.is_null() {
        return std::ptr::null();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.pairs_abi.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

extern "C" fn api_get_pairs_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| (s.pairs_abi.len() / 2) as i32).unwrap_or(0)
}

extern "C" fn api_get_stats(instance: *mut c_void) -> PhysicsStats {
    if instance.is_null() {
        return PhysicsStats::default();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.stats).unwrap_or_default()
}

static PHYSICS_API: PhysicsAPI = PhysicsAPI {
    add_body: api_add_body,
    remove_body: api_remove_body,
    get_body: api_get_body,
    get_bodies_count: api_get_bodies_count,
    update: api_update,
    get_contacts: api_get_contacts,
    get_contacts_count: api_get_contacts_count,
    get_pairs: api_get_pairs,
    get_pairs_count: api_get_pairs_count,
    get_stats: api_get_stats,
};

#[no_mangle]
pub extern "C" fn get_plugin_api() -> PluginAPI {
    PluginAPI {
        version: PLUGIN_API_VERSION,
        plugin_type: PluginType::Physics,
        name: PLUGIN_NAME.as_ptr() as *const c_char,
        init: physics_init,
        shutdown: physics_shutdown,
        update: physics_plugin_update,
        get_physics_api: physics_get_physics_api,
        get_light_api: physics_get_light_api,
    }
}
