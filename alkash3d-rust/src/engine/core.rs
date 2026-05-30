//! Основной движок — связывает рендер, планировщик и DLL

use std::sync::Arc;
use crate::scheduler::*;
use crate::engine::*;

pub struct AlkashEngine {
    pub scheduler: Arc<EngineScheduler>,
    pub physics: Option<PhysicsPlugin>,
    pub lights: Option<LightPlugin>,
}

impl AlkashEngine {
    pub fn new() -> Self {
        Self {
            scheduler: Arc::new(EngineScheduler::new()),
            physics: None,
            lights: None,
        }
    }

    pub fn init_physics(&mut self, config: PhysicsConfig) -> Result<(), String> {
        self.physics = Some(PhysicsPlugin::load("plugins/inertial.dll", config)?);
        Ok(())
    }

    pub fn init_lights(&mut self, device_ptr: *mut std::ffi::c_void, config: LightConfig) -> Result<(), String> {
        self.lights = Some(LightPlugin::load("plugins/firstfires.dll", device_ptr, config)?);
        Ok(())
    }

    /// Синхронное обновление (на главном потоке)
    pub fn update(&mut self, dt: f32, gravity: f32, camera_pos: [f32; 3], view_proj: [f32; 16]) {
        self.scheduler.reset_budget();

        if let Some(physics) = &mut self.physics {
            physics.update(dt, gravity);
        }

        if let Some(lights) = &mut self.lights {
            lights.cull(camera_pos, &view_proj, dt);
        }
    }

    /// Асинхронное обновление — забираем владение плагинами и передаём в потоки
    /// ВНИМАНИЕ: после вызова этого метода плагины становятся недоступны в движке
    pub fn update_async(mut self, dt: f32, gravity: f32, camera_pos: [f32; 3], view_proj: [f32; 16]) -> Vec<std::thread::JoinHandle<()>> {
        let mut handles = Vec::new();

        // Физика в отдельном потоке
        if let Some(physics) = self.physics.take() {
            let (api, instance) = physics.into_async_call();

            let handle = std::thread::spawn(move || {
                (api.update)(instance.as_ptr(), dt, gravity);
            });
            handles.push(handle);
        }

        // Light culling в отдельном потоке
        if let Some(lights) = self.lights.take() {
            let (api, instance) = lights.into_async_call();
            let cam = camera_pos;
            let vp = view_proj;

            let handle = std::thread::spawn(move || {
                (api.cull)(instance.as_ptr(), cam.as_ptr(), vp.as_ptr(), dt);
            });
            handles.push(handle);
        }

        handles
    }

    /// Вариант асинхронного обновления без потери владения (использует Arc+Mutex)
    pub fn update_async_shared(&mut self, dt: f32, gravity: f32, camera_pos: [f32; 3], view_proj: [f32; 16]) -> Vec<std::thread::JoinHandle<()>> {
        let mut handles = Vec::new();

        // Физика — копируем API и указатель (SendPtr копируется)
        if let Some(physics) = &self.physics {
            let api = physics.api;
            let instance = physics.instance;

            let handle = std::thread::spawn(move || {
                (api.update)(instance.as_ptr(), dt, gravity);
            });
            handles.push(handle);
        }

        // Light culling
        if let Some(lights) = &self.lights {
            let api = lights.api;
            let instance = lights.instance;
            let cam = camera_pos;
            let vp = view_proj;

            let handle = std::thread::spawn(move || {
                (api.cull)(instance.as_ptr(), cam.as_ptr(), vp.as_ptr(), dt);
            });
            handles.push(handle);
        }

        handles
    }

    /// Ждём завершения всех асинхронных задач
    pub fn wait_for_tasks(handles: Vec<std::thread::JoinHandle<()>>) {
        for handle in handles {
            let _ = handle.join();
        }
    }

    pub fn add_sphere_body(&mut self, x: f32, y: f32, z: f32, mass: f32) -> Option<i32> {
        let body = FortranRigidBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            inertia: [[0.0; 3]; 3],
            inv_inertia: [[0.0; 3]; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.5,
            friction: 0.5,
            linear_damping: 0.01,
            angular_damping: 0.01,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
        };

        self.physics.as_mut().map(|p| p.add_body(&body))
    }

    pub fn add_street_light(&mut self, x: f32, y: f32, z: f32) -> Option<u32> {
        let light = GPULight {
            position: [x, y, z, 0.0],
            color: [1.0, 0.85, 0.6, 2.5],
            direction: [0.0, -1.0, 0.0, 25.0],
            params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
        };

        self.lights.as_mut().map(|l| l.add_light(&light))
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        self.lights.as_ref().map(|l| l.get_gpu_lights()).unwrap_or(&[])
    }

    pub fn get_contacts(&self) -> &[FortranContact] {
        self.physics.as_ref().map(|p| p.get_contacts()).unwrap_or(&[])
    }
}

impl Default for AlkashEngine {
    fn default() -> Self {
        Self::new()
    }
}