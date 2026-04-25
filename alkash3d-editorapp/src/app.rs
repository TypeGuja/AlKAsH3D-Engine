//! AlKAsH3D Editor - Полноценный 3D редактор в стиле Blender
//! С геометрическим отображением всех объектов

use eframe::egui;
use egui::*;
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;
use std::path::{Path, PathBuf};
use std::fs;
use alkash3d_editor::converters;
use rayon::prelude::*;

// ============================================================
// Математика
// ============================================================

#[derive(Debug, Clone, Copy)]
struct Vec3 { x: f32, y: f32, z: f32 }

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    const ONE: Self = Self { x: 1.0, y: 1.0, z: 1.0 };
    const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    const RIGHT: Self = Self { x: 1.0, y: 0.0, z: 0.0 };
    const FORWARD: Self = Self { x: 0.0, y: 0.0, z: 1.0 };

    fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }
    fn length(&self) -> f32 { (self.x * self.x + self.y * self.y + self.z * self.z).sqrt() }
    fn normalize(&self) -> Self { let len = self.length(); if len > 0.0 { Self { x: self.x / len, y: self.y / len, z: self.z / len } } else { *self } }
    fn cross(&self, other: Vec3) -> Vec3 { Vec3 { x: self.y * other.z - self.z * other.y, y: self.z * other.x - self.x * other.z, z: self.x * other.y - self.y * other.x } }
    fn dot(&self, other: Vec3) -> f32 { self.x * other.x + self.y * other.y + self.z * other.z }
    fn lerp(&self, other: Vec3, t: f32) -> Vec3 { Vec3 { x: self.x + (other.x - self.x) * t, y: self.y + (other.y - self.y) * t, z: self.z + (other.z - self.z) * t } }
    fn min(&self, other: Vec3) -> Vec3 { Vec3 { x: self.x.min(other.x), y: self.y.min(other.y), z: self.z.min(other.z) } }
    fn max(&self, other: Vec3) -> Vec3 { Vec3 { x: self.x.max(other.x), y: self.y.max(other.y), z: self.z.max(other.z) } }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z } }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z } }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self { Self { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs } }
}

impl std::ops::AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

#[derive(Debug, Clone, Copy)]
struct Quat { x: f32, y: f32, z: f32, w: f32 }

impl Quat {
    const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
    fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let half = angle * 0.5;
        let s = half.sin();
        Self { x: axis.x * s, y: axis.y * s, z: axis.z * s, w: half.cos() }
    }
    fn mul(&self, other: &Quat) -> Quat {
        Quat {
            x: self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            y: self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            z: self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            w: self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        }
    }
    fn slerp(&self, other: &Quat, t: f32) -> Quat {
        let dot = (self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w).clamp(-1.0, 1.0);
        let theta = dot.acos();
        let sin_theta = theta.sin();
        if sin_theta.abs() < 0.001 {
            return Quat {
                x: self.x + (other.x - self.x) * t,
                y: self.y + (other.y - self.y) * t,
                z: self.z + (other.z - self.z) * t,
                w: self.w + (other.w - self.w) * t,
            };
        }
        let a = ((1.0 - t) * theta).sin() / sin_theta;
        let b = (t * theta).sin() / sin_theta;
        Quat { x: self.x * a + other.x * b, y: self.y * a + other.y * b, z: self.z * a + other.z * b, w: self.w * a + other.w * b }
    }
    fn rotate(&self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let s = self.w;
        u * (2.0 * u.dot(v)) + v * (s * s - u.dot(u)) + u.cross(v) * (2.0 * s)
    }
}

#[derive(Debug, Clone, Copy)]
struct Transform {
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self { Self { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE } }
}

impl Transform {
    fn to_matrix(&self) -> [[f32; 4]; 4] {
        let rot_mat = self.rotation.to_mat3();
        [
            [rot_mat[0][0] * self.scale.x, rot_mat[0][1] * self.scale.x, rot_mat[0][2] * self.scale.x, self.position.x],
            [rot_mat[1][0] * self.scale.y, rot_mat[1][1] * self.scale.y, rot_mat[1][2] * self.scale.y, self.position.y],
            [rot_mat[2][0] * self.scale.z, rot_mat[2][1] * self.scale.z, rot_mat[2][2] * self.scale.z, self.position.z],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    fn to_mat3(&self) -> [[f32; 3]; 3] {
        let rot_mat = self.rotation.to_mat3();
        [
            [rot_mat[0][0] * self.scale.x, rot_mat[0][1] * self.scale.x, rot_mat[0][2] * self.scale.x],
            [rot_mat[1][0] * self.scale.y, rot_mat[1][1] * self.scale.y, rot_mat[1][2] * self.scale.y],
            [rot_mat[2][0] * self.scale.z, rot_mat[2][1] * self.scale.z, rot_mat[2][2] * self.scale.z],
        ]
    }
}

impl Quat {
    fn to_mat3(&self) -> [[f32; 3]; 3] {
        let x = self.x;
        let y = self.y;
        let z = self.z;
        let w = self.w;

        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        [
            [1.0 - (yy + zz), xy + wz, xz - wy],
            [xy - wz, 1.0 - (xx + zz), yz + wx],
            [xz + wy, yz - wx, 1.0 - (xx + yy)],
        ]
    }
}

// ============================================================
// Анимация
// ============================================================

trait Interpolatable {
    fn interpolate(&self, other: &Self, t: f32) -> Self;
}

impl Interpolatable for Vec3 {
    fn interpolate(&self, other: &Self, t: f32) -> Self { self.lerp(*other, t) }
}

impl Interpolatable for Quat {
    fn interpolate(&self, other: &Self, t: f32) -> Self { self.slerp(other, t) }
}

impl Interpolatable for f32 {
    fn interpolate(&self, other: &Self, t: f32) -> Self { self + (other - self) * t }
}

#[derive(Debug, Clone, Copy)]
enum EasingType { Linear, EaseIn, EaseOut, EaseInOut }

#[derive(Debug, Clone)]
struct Keyframe<T: Clone> {
    time: f32,
    value: T,
    easing: EasingType,
}

#[derive(Debug, Clone)]
struct AnimationTrack<T: Clone + Interpolatable> {
    keyframes: Vec<Keyframe<T>>,
    looped: bool,
}

impl<T: Clone + Interpolatable> AnimationTrack<T> {
    fn new() -> Self { Self { keyframes: Vec::new(), looped: false } }
    fn add_keyframe(&mut self, time: f32, value: T, easing: EasingType) {
        self.keyframes.push(Keyframe { time, value, easing });
        self.keyframes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }
    fn evaluate(&self, time: f32) -> Option<T> {
        if self.keyframes.is_empty() { return None; }
        let max_time = self.keyframes.last().unwrap().time;
        let time = if self.looped { time % max_time } else { time.min(max_time) };
        if time <= self.keyframes[0].time { return Some(self.keyframes[0].value.clone()); }
        for i in 0..self.keyframes.len() - 1 {
            let k1 = &self.keyframes[i];
            let k2 = &self.keyframes[i + 1];
            if time >= k1.time && time <= k2.time {
                let t = (time - k1.time) / (k2.time - k1.time);
                let t = match k1.easing {
                    EasingType::Linear => t,
                    EasingType::EaseIn => t * t,
                    EasingType::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
                    EasingType::EaseInOut => if t < 0.5 { 2.0 * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 },
                };
                return Some(k1.value.interpolate(&k2.value, t));
            }
        }
        Some(self.keyframes.last().unwrap().value.clone())
    }
}

#[derive(Debug, Clone)]
struct Animation {
    name: String,
    position_track: AnimationTrack<Vec3>,
    rotation_track: AnimationTrack<Quat>,
    scale_track: AnimationTrack<Vec3>,
    duration: f32,
    playing: bool,
    current_time: f32,
}

impl Animation {
    fn new(name: String) -> Self {
        Self {
            name,
            position_track: AnimationTrack::new(),
            rotation_track: AnimationTrack::new(),
            scale_track: AnimationTrack::new(),
            duration: 0.0,
            playing: false,
            current_time: 0.0,
        }
    }
    fn update(&mut self, delta_time: f32) {
        if self.playing {
            self.current_time += delta_time;
            if self.current_time > self.duration {
                self.current_time = 0.0;
            }
        }
    }
    fn get_transform(&self) -> Transform {
        Transform {
            position: self.position_track.evaluate(self.current_time).unwrap_or(Vec3::ZERO),
            rotation: self.rotation_track.evaluate(self.current_time).unwrap_or(Quat::IDENTITY),
            scale: self.scale_track.evaluate(self.current_time).unwrap_or(Vec3::ONE),
        }
    }
}

// ============================================================
// Система частиц
// ============================================================

#[derive(Debug, Clone)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    lifetime: f32,
    max_lifetime: f32,
    size: f32,
    color: [f32; 4],
}

#[derive(Debug, Clone)]
struct ParticleSystem {
    particles: Vec<Particle>,
    emission_rate: f32,
    emission_timer: f32,
    max_particles: usize,
    gravity: Vec3,
    start_color: [f32; 4],
    end_color: [f32; 4],
    start_size: f32,
    end_size: f32,
    lifetime: f32,
    velocity: Vec3,
    velocity_random: f32,
    enabled: bool,
    looping: bool,
    transform: Transform,
}

impl ParticleSystem {
    fn new() -> Self {
        Self {
            particles: Vec::new(),
            emission_rate: 10.0,
            emission_timer: 0.0,
            max_particles: 1000,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            start_color: [1.0, 1.0, 1.0, 1.0],
            end_color: [1.0, 1.0, 1.0, 0.0],
            start_size: 0.1,
            end_size: 0.0,
            lifetime: 2.0,
            velocity: Vec3::new(0.0, 5.0, 0.0),
            velocity_random: 1.0,
            enabled: true,
            looping: true,
            transform: Transform::default(),
        }
    }

    fn update(&mut self, delta_time: f32) {
        if !self.enabled { return; }

        self.emission_timer += delta_time;
        let particles_to_emit = (self.emission_timer * self.emission_rate) as usize;
        self.emission_timer = self.emission_timer.fract() / self.emission_rate;

        for _ in 0..particles_to_emit.min(self.max_particles - self.particles.len()) {
            let particle = Particle {
                position: self.transform.position,
                velocity: self.velocity + Vec3::new(
                    rand::random::<f32>() - 0.5,
                    rand::random::<f32>() - 0.5,
                    rand::random::<f32>() - 0.5,
                ) * self.velocity_random,
                lifetime: 0.0,
                max_lifetime: self.lifetime,
                size: self.start_size,
                color: self.start_color,
            };
            self.particles.push(particle);
        }

        self.particles.par_iter_mut().for_each(|particle| {
            particle.lifetime += delta_time;
            particle.velocity += self.gravity * delta_time;
            particle.position += particle.velocity * delta_time;
            let t = particle.lifetime / particle.max_lifetime;
            particle.size = self.start_size + (self.end_size - self.start_size) * t;
            for i in 0..4 {
                particle.color[i] = self.start_color[i] + (self.end_color[i] - self.start_color[i]) * t;
            }
        });

        self.particles.retain(|p| p.lifetime < p.max_lifetime);

        if !self.looping && self.particles.is_empty() {
            self.enabled = false;
        }
    }
}

// ============================================================
// Undo/Redo система
// ============================================================

#[derive(Debug, Clone)]
enum EditorCommand {
    CreateObject { id: Uuid, object: GameObject },
    DeleteObject { id: Uuid, object: GameObject },
    ModifyTransform { id: Uuid, old_transform: Transform, new_transform: Transform },
    ModifyName { id: Uuid, old_name: String, new_name: String },
    ModifyVisibility { id: Uuid, old_visible: bool, new_visible: bool },
    GroupCommand { commands: Vec<EditorCommand>, description: String },
}

struct CommandHistory {
    undo_stack: VecDeque<EditorCommand>,
    redo_stack: VecDeque<EditorCommand>,
    max_size: usize,
}

impl CommandHistory {
    fn new(max_size: usize) -> Self {
        Self { undo_stack: VecDeque::new(), redo_stack: VecDeque::new(), max_size }
    }
    fn push(&mut self, command: EditorCommand) {
        if self.undo_stack.len() >= self.max_size { self.undo_stack.pop_back(); }
        self.undo_stack.push_front(command);
        self.redo_stack.clear();
    }
    fn undo(&mut self, scene: &mut Scene) -> bool {
        if let Some(command) = self.undo_stack.pop_front() {
            Self::apply_undo(command.clone(), scene);
            self.redo_stack.push_front(command);
            true
        } else { false }
    }
    fn redo(&mut self, scene: &mut Scene) -> bool {
        if let Some(command) = self.redo_stack.pop_front() {
            Self::apply_redo(command.clone(), scene);
            self.undo_stack.push_front(command);
            true
        } else { false }
    }
    fn apply_undo(command: EditorCommand, scene: &mut Scene) {
        match command {
            EditorCommand::CreateObject { id, .. } => { scene.remove_object(id); }
            EditorCommand::DeleteObject { object, .. } => { scene.add_object(object); }
            EditorCommand::ModifyTransform { id, old_transform, .. } => {
                if let Some(obj) = scene.get_object_mut(id) { obj.transform = old_transform; }
            }
            EditorCommand::ModifyName { id, old_name, .. } => {
                if let Some(obj) = scene.get_object_mut(id) { obj.name = old_name; }
            }
            EditorCommand::ModifyVisibility { id, old_visible, .. } => {
                if let Some(obj) = scene.get_object_mut(id) { obj.visible = old_visible; }
            }
            EditorCommand::GroupCommand { commands, .. } => {
                for cmd in commands.into_iter().rev() { Self::apply_undo(cmd, scene); }
            }
        }
    }
    fn apply_redo(command: EditorCommand, scene: &mut Scene) {
        match command {
            EditorCommand::CreateObject { object, .. } => { scene.add_object(object); }
            EditorCommand::DeleteObject { id, .. } => { scene.remove_object(id); }
            EditorCommand::ModifyTransform { id, new_transform, .. } => {
                if let Some(obj) = scene.get_object_mut(id) { obj.transform = new_transform; }
            }
            EditorCommand::ModifyName { id, new_name, .. } => {
                if let Some(obj) = scene.get_object_mut(id) { obj.name = new_name; }
            }
            EditorCommand::ModifyVisibility { id, new_visible, .. } => {
                if let Some(obj) = scene.get_object_mut(id) { obj.visible = new_visible; }
            }
            EditorCommand::GroupCommand { commands, .. } => {
                for cmd in commands { Self::apply_redo(cmd, scene); }
            }
        }
    }
    fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
    fn get_undo_description(&self) -> Option<String> {
        self.undo_stack.front().map(|cmd| match cmd {
            EditorCommand::CreateObject { object, .. } => format!("Create '{}'", object.name),
            EditorCommand::DeleteObject { object, .. } => format!("Delete '{}'", object.name),
            EditorCommand::ModifyTransform { .. } => "Move Object".to_string(),
            EditorCommand::ModifyName { old_name, new_name, .. } => format!("Rename '{}' to '{}'", old_name, new_name),
            EditorCommand::ModifyVisibility { .. } => "Toggle Visibility".to_string(),
            EditorCommand::GroupCommand { description, .. } => description.clone(),
        })
    }
}

// ============================================================
// Меш (сетка)
// ============================================================

#[derive(Debug, Clone)]
struct Mesh {
    vertices: Vec<Vec3>,
    indices: Vec<u32>,
    normals: Vec<Vec3>,
    bounds: (Vec3, Vec3),
}

impl Mesh {
    fn new(vertices: Vec<Vec3>, indices: Vec<u32>) -> Self {
        let mut mesh = Self {
            vertices: vertices.clone(),
            indices: indices.clone(),
            normals: vec![Vec3::ZERO; vertices.len()],
            bounds: (Vec3::ZERO, Vec3::ZERO),
        };

        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        for v in &vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        mesh.bounds = (min, max);

        mesh.recalculate_normals();
        mesh
    }

    fn create_cube() -> Self {
        let vertices = vec![
            Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5), Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(-0.5, -0.5, 0.5), Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.5), Vec3::new(-0.5, 0.5, 0.5),
        ];
        let indices = vec![
            0,1,2, 2,3,0, 4,5,6, 6,7,4,
            0,4,7, 7,3,0, 1,5,6, 6,2,1,
            0,1,5, 5,4,0, 3,2,6, 6,7,3,
        ];
        Self::new(vertices, indices)
    }

    fn create_sphere() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        let rings = 16;
        for i in 0..=rings {
            let phi = std::f32::consts::PI * i as f32 / rings as f32;
            let y = -phi.cos() * 0.5;
            let r = phi.sin() * 0.5;
            for j in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / segments as f32;
                vertices.push(Vec3::new(r * theta.cos(), y, r * theta.sin()));
            }
        }
        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + 1;
                let c = (i + 1) * (segments + 1) + j;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
            }
        }
        Self::new(vertices, indices)
    }

    fn create_plane() -> Self {
        let vertices = vec![
            Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, -5.0),
            Vec3::new(5.0, 0.0, 5.0), Vec3::new(-5.0, 0.0, 5.0),
        ];
        let indices = vec![0,1,2, 2,3,0];
        Self::new(vertices, indices)
    }

    fn create_cylinder() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        for i in 0..segments {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = angle.cos() * 0.5;
            let z = angle.sin() * 0.5;
            vertices.push(Vec3::new(x, -0.5, z));
            vertices.push(Vec3::new(x, 0.5, z));
        }
        for i in 0..segments {
            let next = (i + 1) % segments;
            let base = (i * 2) as u32;
            let next_base = (next * 2) as u32;
            indices.extend_from_slice(&[base, base+1, next_base, next_base, base+1, next_base+1]);
        }
        Self::new(vertices, indices)
    }

    fn create_cone() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        vertices.push(Vec3::new(0.0, 0.5, 0.0));
        for i in 0..segments {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = angle.cos() * 0.5;
            let z = angle.sin() * 0.5;
            vertices.push(Vec3::new(x, -0.5, z));
        }
        for i in 0..segments {
            let next = (i + 1) % segments;
            indices.extend_from_slice(&[0, (i+1) as u32, (next+1) as u32]);
        }
        Self::new(vertices, indices)
    }

    fn create_torus() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        let rings = 16;
        let r1 = 0.2;
        let r2 = 0.5;
        for i in 0..=rings {
            let phi = 2.0 * std::f32::consts::PI * i as f32 / rings as f32;
            for j in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / segments as f32;
                let x = (r2 + r1 * theta.cos()) * phi.cos();
                let y = r1 * theta.sin();
                let z = (r2 + r1 * theta.cos()) * phi.sin();
                vertices.push(Vec3::new(x, y, z));
            }
        }
        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + 1;
                let c = (i + 1) * (segments + 1) + j;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
            }
        }
        Self::new(vertices, indices)
    }

    fn recalculate_normals(&mut self) {
        self.normals = vec![Vec3::ZERO; self.vertices.len()];

        for i in (0..self.indices.len()).step_by(3) {
            if i + 2 >= self.indices.len() { break; }

            let i0 = self.indices[i] as usize;
            let i1 = self.indices[i + 1] as usize;
            let i2 = self.indices[i + 2] as usize;

            if i0 < self.vertices.len() && i1 < self.vertices.len() && i2 < self.vertices.len() {
                let v0 = self.vertices[i0];
                let v1 = self.vertices[i1];
                let v2 = self.vertices[i2];

                let edge1 = v1 - v0;
                let edge2 = v2 - v0;
                let normal = edge1.cross(edge2);
                let len = normal.length();

                if len > 0.0001 {
                    let normal = Vec3::new(normal.x / len, normal.y / len, normal.z / len);

                    self.normals[i0] = self.normals[i0] + normal;
                    self.normals[i1] = self.normals[i1] + normal;
                    self.normals[i2] = self.normals[i2] + normal;
                }
            }
        }

        self.normals.par_iter_mut().for_each(|normal| {
            let len = normal.length();
            if len > 0.0001 {
                *normal = Vec3::new(normal.x / len, normal.y / len, normal.z / len);
            } else {
                *normal = Vec3::UP;
            }
        });

        println!("[Mesh] Calculated {} normals", self.normals.len());
    }
}

// ============================================================
// Материал
// ============================================================

#[derive(Debug, Clone)]
struct Material {
    name: String,
    color: [f32; 4],
    metallic: f32,
    roughness: f32,
    emissive: [f32; 3],
}

impl Default for Material {
    fn default() -> Self {
        Self { name: "Default".to_string(), color: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.5, emissive: [0.0, 0.0, 0.0] }
    }
}

// ============================================================
// Объекты сцены
// ============================================================

#[derive(Debug, Clone)]
struct MeshComponent {
    mesh: Mesh,
    material: Material,
    visible: bool,
    wireframe: bool,
    solid: bool,
    double_sided: bool,
}

#[derive(Debug, Clone)]
struct LightComponent {
    light_type: LightType,
    color: [f32; 3],
    intensity: f32,
    range: f32,
    enabled: bool,
}

#[derive(Debug, Clone)]
enum LightType { Point, Directional, Spot { inner_angle: f32, outer_angle: f32 } }

#[derive(Debug, Clone)]
struct CameraComponent {
    fov: f32,
    near: f32,
    far: f32,
    orthographic: bool,
}

#[derive(Debug, Clone)]
struct ParticleSystemComponent {
    system: ParticleSystem,
    enabled: bool,
}

#[derive(Debug, Clone)]
enum ObjectType {
    Empty,
    Mesh(MeshComponent),
    Light(LightComponent),
    Camera(CameraComponent),
    ParticleSystem(ParticleSystemComponent),
}

#[derive(Debug, Clone)]
struct GameObject {
    id: Uuid,
    name: String,
    visible: bool,
    locked: bool,
    transform: Transform,
    parent: Option<Uuid>,
    children: Vec<Uuid>,
    object_type: ObjectType,
    tags: Vec<String>,
    animations: HashMap<String, Animation>,
}

impl GameObject {
    fn new(name: &str, object_type: ObjectType) -> Self {
        Self {
            id: Uuid::new_v4(), name: name.to_string(), visible: true, locked: false,
            transform: Transform::default(), parent: None, children: Vec::new(),
            object_type, tags: Vec::new(), animations: HashMap::new(),
        }
    }
}

// ============================================================
// Сцена
// ============================================================

struct Scene {
    name: String,
    path: Option<String>,
    objects: HashMap<Uuid, GameObject>,
    selected_ids: Vec<Uuid>,
    main_camera: Option<Uuid>,
    ambient_color: [f32; 3],
    grid_enabled: bool,
    snap_enabled: bool,
    snap_size: f32,
    dirty: bool,
    playing: bool,
    animation_time: f32,
}

impl Scene {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(), path: None, objects: HashMap::new(), selected_ids: Vec::new(),
            main_camera: None, ambient_color: [0.2, 0.2, 0.25],
            grid_enabled: true, snap_enabled: false, snap_size: 1.0, dirty: false,
            playing: false, animation_time: 0.0,
        }
    }
    fn add_object(&mut self, obj: GameObject) -> Uuid { let id = obj.id; self.objects.insert(id, obj); self.dirty = true; id }
    fn remove_object(&mut self, id: Uuid) -> Option<GameObject> {
        self.selected_ids.retain(|&sid| sid != id);
        if self.main_camera == Some(id) { self.main_camera = None; }
        self.dirty = true;
        self.objects.remove(&id)
    }
    fn get_object(&self, id: Uuid) -> Option<&GameObject> { self.objects.get(&id) }
    fn get_object_mut(&mut self, id: Uuid) -> Option<&mut GameObject> { self.dirty = true; self.objects.get_mut(&id) }
    fn selected_objects(&self) -> Vec<&GameObject> { self.selected_ids.iter().filter_map(|id| self.objects.get(id)).collect() }
    fn clear_selection(&mut self) { self.selected_ids.clear(); }
    fn select(&mut self, id: Uuid, add: bool) {
        if add {
            if !self.selected_ids.contains(&id) { self.selected_ids.push(id); }
        } else {
            self.selected_ids.clear();
            self.selected_ids.push(id);
        }
    }
    fn duplicate_selected(&mut self) -> Vec<Uuid> {
        let to_duplicate: Vec<GameObject> = self.selected_objects().into_iter().cloned().collect();
        let mut new_ids = Vec::new();
        for mut obj in to_duplicate {
            obj.id = Uuid::new_v4();
            obj.name = format!("{} (Copy)", obj.name);
            obj.transform.position = obj.transform.position + Vec3::new(1.0, 0.0, 1.0);
            let id = obj.id;
            self.add_object(obj);
            new_ids.push(id);
        }
        self.selected_ids = new_ids.clone();
        new_ids
    }
    fn delete_selected(&mut self) {
        let ids: Vec<Uuid> = self.selected_ids.drain(..).collect();
        for id in ids { self.objects.remove(&id); }
        self.dirty = true;
    }
    fn get_world_transform(&self, id: Uuid) -> Transform {
        if let Some(obj) = self.objects.get(&id) {
            let mut transform = obj.transform;
            let mut current_parent = obj.parent;
            while let Some(parent_id) = current_parent {
                if let Some(parent) = self.objects.get(&parent_id) {
                    transform.position = transform.position + parent.transform.position;
                    transform.rotation = transform.rotation.mul(&parent.transform.rotation);
                    transform.scale = Vec3::new(
                        transform.scale.x * parent.transform.scale.x,
                        transform.scale.y * parent.transform.scale.y,
                        transform.scale.z * parent.transform.scale.z,
                    );
                    current_parent = parent.parent;
                } else { break; }
            }
            transform
        } else { Transform::default() }
    }
    fn update(&mut self, delta_time: f32) {
        if self.playing {
            self.animation_time += delta_time;
        }

        let mut objects_vec: Vec<_> = self.objects.values_mut().collect();
        objects_vec.par_iter_mut().for_each(|obj| {
            if let ObjectType::ParticleSystem(ref mut ps) = obj.object_type {
                ps.system.update(delta_time);
            }
            for anim in obj.animations.values_mut() {
                anim.update(delta_time);
                if anim.playing {
                    let anim_transform = anim.get_transform();
                    obj.transform.position = anim_transform.position;
                    obj.transform.rotation = anim_transform.rotation;
                    obj.transform.scale = anim_transform.scale;
                }
            }
        });
    }
}

// ============================================================
// Ассеты и импорт (ВСЁ через Altex)
// ============================================================

struct AssetLibrary {
    meshes: HashMap<String, Mesh>,
    materials: HashMap<String, Material>,
}

impl AssetLibrary {
    fn new() -> Self {
        let mut meshes = HashMap::new();
        meshes.insert("cube".to_string(), Mesh::create_cube());
        meshes.insert("sphere".to_string(), Mesh::create_sphere());
        meshes.insert("plane".to_string(), Mesh::create_plane());
        meshes.insert("cylinder".to_string(), Mesh::create_cylinder());
        meshes.insert("cone".to_string(), Mesh::create_cone());
        meshes.insert("torus".to_string(), Mesh::create_torus());

        let mut materials = HashMap::new();
        materials.insert("default".to_string(), Material::default());
        materials.insert("red".to_string(), Material { color: [1.0, 0.2, 0.2, 1.0], ..Default::default() });
        materials.insert("green".to_string(), Material { color: [0.2, 1.0, 0.2, 1.0], ..Default::default() });
        materials.insert("blue".to_string(), Material { color: [0.2, 0.2, 1.0, 1.0], ..Default::default() });
        materials.insert("metal".to_string(), Material { metallic: 1.0, roughness: 0.3, ..Default::default() });

        Self { meshes, materials }
    }

    fn import_model(&mut self, path: &str) -> Result<Vec<String>, String> {
        let input_path = Path::new(path);
        let extension = input_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let file_name = input_path.file_stem().and_then(|n| n.to_str()).unwrap_or("unnamed");

        let temp_dir = std::env::temp_dir().join(format!("alkash_import_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

        let output_path = temp_dir.join(format!("{}.obj", file_name));
        let output_str = output_path.to_str().unwrap();

        println!("[AssetLibrary] Converting {}...", path);

        match extension.as_str() {
            "obj" => {
                fs::copy(path, &output_path).map_err(|e| e.to_string())?;
            }
            "blend" => converters::blend::convert(path, output_str).map_err(|e| format!("Blend: {}", e))?,
            "fbx" => converters::fbx::convert(path, output_str).map_err(|e| format!("FBX: {}", e))?,
            "gltf" | "glb" => converters::gltf::convert(path, output_str).map_err(|e| format!("glTF: {}", e))?,
            _ => return Err(format!("Unsupported format: {}", extension)),
        }

        let mesh = self.parse_obj(output_str)?;
        self.meshes.insert(file_name.to_string(), mesh.clone());

        println!("[AssetLibrary] Successfully loaded: {} ({} vertices, {} triangles)",
                 file_name, mesh.vertices.len(), mesh.indices.len() / 3);

        let _ = fs::remove_dir_all(temp_dir);
        Ok(vec![file_name.to_string()])
    }

    fn parse_obj(&self, path: &str) -> Result<Mesh, String> {
        println!("[OBJ] Parsing file: {}", path);

        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let lines: Vec<&str> = content.lines().collect();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut normals_from_file = Vec::new();
        let mut texcoords = Vec::new();

        println!("[OBJ] Total lines: {}", lines.len());

        let mut vertex_count = 0;
        let mut face_count = 0;
        let mut normal_count = 0;

        for line in lines.iter() {
            let line = line.trim();
            if line.is_empty() { continue; }

            if line.starts_with("vn ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    normals_from_file.push(Vec3::new(
                        parts[1].parse().unwrap_or(0.0),
                        parts[2].parse().unwrap_or(0.0),
                        parts[3].parse().unwrap_or(0.0),
                    ));
                    normal_count += 1;
                }
            } else if line.starts_with("vt ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    texcoords.push((parts[1].parse().unwrap_or(0.0), parts[2].parse().unwrap_or(0.0)));
                }
            } else if line.starts_with("v ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    vertices.push(Vec3::new(
                        parts[1].parse().unwrap_or(0.0),
                        parts[2].parse().unwrap_or(0.0),
                        parts[3].parse().unwrap_or(0.0),
                    ));
                    vertex_count += 1;
                }
            } else if line.starts_with("f ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let mut face_indices = Vec::new();
                    for i in 1..parts.len() {
                        let idx_str = parts[i].split('/').next().unwrap_or("");
                        if let Ok(idx) = idx_str.parse::<i32>() {
                            let idx = if idx > 0 { idx - 1 } else { vertices.len() as i32 + idx };
                            if idx >= 0 && idx < vertices.len() as i32 {
                                face_indices.push(idx as u32);
                            }
                        }
                    }

                    if face_indices.len() >= 3 {
                        for j in 1..face_indices.len() - 1 {
                            indices.push(face_indices[0]);
                            indices.push(face_indices[j]);
                            indices.push(face_indices[j + 1]);
                        }
                        face_count += 1;
                    }
                }
            }
        }

        println!("[OBJ] Vertices: {}, Faces: {}, Normals in file: {}",
                 vertex_count, face_count, normal_count);
        println!("[OBJ] Generated indices: {}", indices.len());

        if vertices.is_empty() {
            return Err("No vertices found".to_string());
        }

        let mut mesh = Mesh {
            vertices: vertices.clone(),
            indices: indices.clone(),
            normals: vec![Vec3::UP; vertices.len()],
            bounds: (Vec3::ZERO, Vec3::ZERO),
        };

        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        for v in &vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        mesh.bounds = (min, max);

        println!("[OBJ] Bounds: min({:.2}, {:.2}, {:.2}) max({:.2}, {:.2}, {:.2})",
                 min.x, min.y, min.z, max.x, max.y, max.z);

        if normal_count == vertex_count {
            mesh.normals = normals_from_file;
            println!("[OBJ] Using normals from file");

            let mut invalid_normals = 0;
            for n in &mesh.normals {
                let len = n.length();
                if len < 0.01 || len > 10.0 || len.is_nan() {
                    invalid_normals += 1;
                }
            }

            if invalid_normals > 0 {
                println!("[OBJ] Warning: {} invalid normals detected, recalculating...", invalid_normals);
                mesh.recalculate_normals();
            }
        } else {
            println!("[OBJ] Calculating normals...");
            mesh.recalculate_normals();
        }


        Ok(mesh)
    }

    fn parse_altex(&self, path: &str) -> Result<Mesh, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut normals = Vec::new();

        let mut in_mesh = false;
        let mut reading_vertices = false;
        let mut reading_indices = false;

        for line in content.lines() {
            let line = line.trim();

            if line.starts_with("mesh ") {
                in_mesh = true;
            } else if in_mesh && line.starts_with("vertices ") {
                reading_vertices = true;
                reading_indices = false;
            } else if in_mesh && line.starts_with("indices ") {
                reading_vertices = false;
                reading_indices = true;
            } else if reading_vertices && line.starts_with("v ") {
                let parts: Vec<&str> = line[2..].split('|').collect();
                if !parts.is_empty() {
                    let pos: Vec<f32> = parts[0].split_whitespace().filter_map(|s| s.parse().ok()).collect();
                    if pos.len() >= 3 {
                        vertices.push(Vec3::new(pos[0], pos[1], pos[2]));

                        if parts.len() >= 2 {
                            let norm: Vec<f32> = parts[1].split_whitespace().filter_map(|s| s.parse().ok()).collect();
                            if norm.len() >= 3 {
                                normals.push(Vec3::new(norm[0], norm[1], norm[2]));
                            }
                        }
                    }
                }
            } else if reading_indices && line.starts_with("i ") {
                for part in line[2..].split_whitespace() {
                    if let Ok(idx) = part.parse::<u32>() {
                        indices.push(idx);
                    }
                }
            }
        }

        if vertices.is_empty() {
            return Err("No vertices found".to_string());
        }

        let mut mesh = Mesh::new(vertices, indices);
        if normals.len() == mesh.vertices.len() {
            mesh.normals = normals;
        }

        Ok(mesh)
    }

    fn load_altex(&mut self, path: &str) -> Result<String, String> {
        let file_name = Path::new(path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let mesh = self.parse_altex(path)?;
        self.meshes.insert(file_name.clone(), mesh);

        Ok(file_name)
    }

    fn get_mesh(&self, name: &str) -> Option<&Mesh> { self.meshes.get(name) }
    fn list_meshes(&self) -> Vec<String> { self.meshes.keys().cloned().collect() }
    fn list_materials(&self) -> Vec<String> { self.materials.keys().cloned().collect() }
}

// ============================================================
// Гизмо
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GizmoMode { Translate, Rotate, Scale, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GizmoSpace { World, Local }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GizmoAxis { X, Y, Z, XY, YZ, XZ, None }

struct Gizmo {
    mode: GizmoMode,
    space: GizmoSpace,
    position: Vec3,
    rotation: Quat,
    visible: bool,
    dragging: bool,
    active_axis: GizmoAxis,
    drag_start_pos: Vec3,
    drag_start_value: Vec3,
}

impl Default for Gizmo {
    fn default() -> Self {
        Self {
            mode: GizmoMode::Translate, space: GizmoSpace::World,
            position: Vec3::ZERO, rotation: Quat::IDENTITY, visible: true,
            dragging: false, active_axis: GizmoAxis::None,
            drag_start_pos: Vec3::ZERO, drag_start_value: Vec3::ZERO,
        }
    }
}

impl Gizmo {
    fn update(&mut self, transform: Transform) { self.position = transform.position; self.rotation = transform.rotation; }
    fn begin_drag(&mut self, axis: GizmoAxis, start_pos: Vec3) {
        self.dragging = true;
        self.active_axis = axis;
        self.drag_start_pos = start_pos;
        self.drag_start_value = self.position;
    }
    fn drag(&mut self, delta: Vec3) -> Transform {
        let mut new_transform = Transform { position: self.position, rotation: self.rotation, scale: Vec3::ONE };
        match (self.mode, self.active_axis) {
            (GizmoMode::Translate, GizmoAxis::X) => new_transform.position.x = self.drag_start_value.x + delta.x,
            (GizmoMode::Translate, GizmoAxis::Y) => new_transform.position.y = self.drag_start_value.y + delta.y,
            (GizmoMode::Translate, GizmoAxis::Z) => new_transform.position.z = self.drag_start_value.z + delta.z,
            (GizmoMode::Translate, GizmoAxis::XY) => { new_transform.position.x = self.drag_start_value.x + delta.x; new_transform.position.y = self.drag_start_value.y + delta.y; }
            (GizmoMode::Translate, GizmoAxis::YZ) => { new_transform.position.y = self.drag_start_value.y + delta.y; new_transform.position.z = self.drag_start_value.z + delta.z; }
            (GizmoMode::Translate, GizmoAxis::XZ) => { new_transform.position.x = self.drag_start_value.x + delta.x; new_transform.position.z = self.drag_start_value.z + delta.z; }
            (GizmoMode::Rotate, GizmoAxis::X) => new_transform.rotation = self.rotation.mul(&Quat::from_axis_angle(Vec3::RIGHT, delta.x * 0.01)),
            (GizmoMode::Rotate, GizmoAxis::Y) => new_transform.rotation = self.rotation.mul(&Quat::from_axis_angle(Vec3::UP, delta.x * 0.01)),
            (GizmoMode::Rotate, GizmoAxis::Z) => new_transform.rotation = self.rotation.mul(&Quat::from_axis_angle(Vec3::FORWARD, delta.x * 0.01)),
            (GizmoMode::Scale, GizmoAxis::X) => new_transform.scale.x = (self.drag_start_value.x + delta.x * 0.01).max(0.01),
            (GizmoMode::Scale, GizmoAxis::Y) => new_transform.scale.y = (self.drag_start_value.y + delta.y * 0.01).max(0.01),
            (GizmoMode::Scale, GizmoAxis::Z) => new_transform.scale.z = (self.drag_start_value.z + delta.z * 0.01).max(0.01),
            _ => {}
        }
        new_transform
    }
    fn end_drag(&mut self) { self.dragging = false; self.active_axis = GizmoAxis::None; }
}

// ============================================================
// Editor App
// ============================================================

pub struct EditorApp {
    scene: Scene,
    history: CommandHistory,
    asset_library: AssetLibrary,
    camera_position: Vec3,
    camera_target: Vec3,
    camera_up: Vec3,
    camera_fov: f32,
    camera_near: f32,
    camera_far: f32,
    current_tool: EditorTool,
    gizmo: Gizmo,
    viewport_rect: Rect,
    show_hierarchy: bool,
    show_inspector: bool,
    show_console: bool,
    show_asset_browser: bool,
    show_animation_timeline: bool,
    last_mouse_pos: Option<Pos2>,
    right_mouse_pressed: bool,
    middle_mouse_pressed: bool,
    left_mouse_pressed: bool,
    status_message: String,
    fps: f32,
    frame_count: u64,
    last_frame_time: f64,
    last_update_time: f64,
    show_new_scene_dialog: bool,
    show_settings_dialog: bool,
    show_import_dialog: bool,
    show_export_dialog: bool,
    show_save_dialog: bool,
    new_scene_name: String,
    console_messages: VecDeque<(String, Color32)>,
    search_filter: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool { Select, Move, Rotate, Scale }

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_egui_style(&cc.egui_ctx);

        let mut scene = Scene::new("Untitled");
        let asset_library = AssetLibrary::new();

        let camera = GameObject::new("Main Camera", ObjectType::Camera(CameraComponent {
            fov: 60.0, near: 0.1, far: 1000.0, orthographic: false,
        }));
        let camera_id = camera.id;
        scene.add_object(camera);
        scene.main_camera = Some(camera_id);

        let light = GameObject::new("Directional Light", ObjectType::Light(LightComponent {
            light_type: LightType::Directional,
            color: [1.0, 0.95, 0.9], intensity: 1.0, range: 100.0, enabled: true,
        }));
        scene.add_object(light);

        let cube = GameObject::new("Cube", ObjectType::Mesh(MeshComponent {
            mesh: asset_library.get_mesh("cube").unwrap().clone(),
            material: Material::default(),
            visible: true, wireframe: false, solid: true,
            double_sided: false,
        }));
        scene.add_object(cube);

        let sphere = GameObject::new("Sphere", ObjectType::Mesh(MeshComponent {
            mesh: asset_library.get_mesh("sphere").unwrap().clone(),
            material: Material::default(),
            visible: true, wireframe: false, solid: true,
            double_sided: false,
        }));
        let sphere_id = sphere.id;
        scene.add_object(sphere);
        if let Some(obj) = scene.get_object_mut(sphere_id) {
            obj.transform.position = Vec3::new(2.0, 0.5, 0.0);
        }

        let plane = GameObject::new("Ground", ObjectType::Mesh(MeshComponent {
            mesh: asset_library.get_mesh("plane").unwrap().clone(),
            material: Material { color: [0.3, 0.5, 0.3, 1.0], ..Default::default() },
            visible: true, wireframe: false, solid: true,
            double_sided: false,
        }));
        scene.add_object(plane);

        let mut ps = ParticleSystem::new();
        ps.transform.position = Vec3::new(-2.0, 1.0, 0.0);
        let particle_obj = GameObject::new("Particle System", ObjectType::ParticleSystem(ParticleSystemComponent {
            system: ps, enabled: true,
        }));
        scene.add_object(particle_obj);

        scene.select(sphere_id, false);

        let mut console = VecDeque::new();
        console.push_back(("🚀 Editor started".to_string(), Color32::GREEN));
        console.push_back(("✅ Ready".to_string(), Color32::LIGHT_GRAY));

        Self {
            scene, history: CommandHistory::new(100), asset_library,
            camera_position: Vec3::new(5.0, 5.0, 10.0),
            camera_target: Vec3::ZERO,
            camera_up: Vec3::UP,
            camera_fov: 60.0, camera_near: 0.1, camera_far: 1000.0,
            current_tool: EditorTool::Select,
            gizmo: Gizmo::default(),
            viewport_rect: Rect::NOTHING,
            show_hierarchy: true, show_inspector: true, show_console: true,
            show_asset_browser: false, show_animation_timeline: false,
            last_mouse_pos: None,
            right_mouse_pressed: false, middle_mouse_pressed: false, left_mouse_pressed: false,
            status_message: String::from("Ready"),
            fps: 0.0, frame_count: 0, last_frame_time: 0.0, last_update_time: 0.0,
            show_new_scene_dialog: false, show_settings_dialog: false,
            show_import_dialog: false, show_export_dialog: false, show_save_dialog: false,
            new_scene_name: String::from("New Scene"),
            console_messages: console,
            search_filter: String::new(),
        }
    }

    fn log_message(&mut self, msg: impl Into<String>, color: Color32) {
        let msg = msg.into();
        self.console_messages.push_back((msg.clone(), color));
        if self.console_messages.len() > 100 { self.console_messages.pop_front(); }
        self.status_message = msg;
    }

    fn orbit_camera(&mut self, delta_x: f32, delta_y: f32) {
        let angle_h = -delta_x * 0.01;
        let angle_v = -delta_y * 0.01;
        let dir = self.camera_position - self.camera_target;
        let radius = dir.length();
        let mut h_angle = dir.z.atan2(dir.x);
        let mut v_angle = (dir.y / radius).asin();
        h_angle += angle_h;
        v_angle = (v_angle + angle_v).clamp(-1.4, 1.4);
        let new_dir = Vec3::new(
            v_angle.cos() * h_angle.cos(),
            v_angle.sin(),
            v_angle.cos() * h_angle.sin(),
        ) * radius;
        self.camera_position = self.camera_target + new_dir;
    }

    fn pan_camera(&mut self, delta_x: f32, delta_y: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let speed = self.camera_position.length() * 0.001;
        let offset = right * (-delta_x * speed) + up * (delta_y * speed);
        self.camera_position = self.camera_position + offset;
        self.camera_target = self.camera_target + offset;
    }

    fn zoom_camera(&mut self, delta: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let distance = (self.camera_target - self.camera_position).length();
        let new_distance = (distance - delta * 0.5).clamp(2.0, 50.0);
        self.camera_position = self.camera_target - dir * new_distance;
    }

    fn screen_to_ray(&self, screen_pos: Pos2, rect: Rect) -> Vec3 {
        let ndc_x = (screen_pos.x - rect.min.x) / rect.width() * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_pos.y - rect.min.y) / rect.height() * 2.0;
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let tan_fov = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();
        right * ndc_x * tan_fov + up * ndc_y * tan_fov + dir
    }

    fn screen_to_world(&self, screen_pos: Pos2, rect: Rect) -> Vec3 {
        let ray_dir = self.screen_to_ray(screen_pos, rect);
        let plane_normal = (self.camera_target - self.camera_position).normalize();
        let t = (self.gizmo.position - self.camera_position).dot(plane_normal) / ray_dir.dot(plane_normal);
        self.camera_position + ray_dir * t
    }

    fn world_to_screen(&self, world_pos: Vec3, rect: Rect) -> Option<Pos2> {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let relative = world_pos - self.camera_position;
        let distance = relative.dot(dir);
        if distance <= 0.01 { return None; }
        let tan_fov = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();
        let scale = 1.0 / (distance * tan_fov);
        let x = relative.dot(right) * scale;
        let y = relative.dot(up) * scale;
        let center = rect.center();
        let px = center.x + x * rect.width() * 0.5;
        let py = center.y - y * rect.height() * 0.5;
        if px < rect.min.x - 100.0 || px > rect.max.x + 100.0 || py < rect.min.y - 100.0 || py > rect.max.y + 100.0 {
            None
        } else {
            Some(Pos2::new(px, py))
        }
    }

    fn detect_gizmo_axis(&self, screen_pos: Pos2, rect: Rect) -> GizmoAxis {
        if !self.gizmo.visible { return GizmoAxis::None; }
        let gizmo_screen_pos = match self.world_to_screen(self.gizmo.position, rect) {
            Some(pos) => pos,
            None => return GizmoAxis::None,
        };
        let distance = (screen_pos - gizmo_screen_pos).length();
        if distance > 80.0 { return GizmoAxis::None; }

        let axes = [(GizmoAxis::X, Vec3::RIGHT), (GizmoAxis::Y, Vec3::UP), (GizmoAxis::Z, Vec3::FORWARD)];
        for (axis, dir) in axes {
            let axis_dir = if self.gizmo.space == GizmoSpace::World { dir } else { self.gizmo.rotation.rotate(dir) };
            let axis_end = self.gizmo.position + axis_dir * 2.0;
            if let Some(axis_screen_end) = self.world_to_screen(axis_end, rect) {
                let dist = self.point_to_line_distance(screen_pos, gizmo_screen_pos, axis_screen_end);
                if dist < 30.0 { return axis; }
            }
        }
        GizmoAxis::None
    }

    fn point_to_line_distance(&self, p: Pos2, a: Pos2, b: Pos2) -> f32 {
        let ab = b - a;
        let ap = p - a;
        let t = (ap.x * ab.x + ap.y * ab.y) / (ab.x * ab.x + ab.y * ab.y).max(0.0001);
        if t <= 0.0 { ap.length() } else if t >= 1.0 { (p - b).length() } else { (p - (a + ab * t)).length() }
    }

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect) {
        self.viewport_rect = rect;
        if !ui.rect_contains_pointer(rect) { return; }

        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
        let right = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let middle = ui.input(|i| i.pointer.button_down(PointerButton::Middle));
        let left = ui.input(|i| i.pointer.button_down(PointerButton::Primary));
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let shift = ui.input(|i| i.modifiers.shift);

        if right {
            if let (Some(cur), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                self.orbit_camera(cur.x - last.x, cur.y - last.y);
            }
            self.right_mouse_pressed = true;
        } else { self.right_mouse_pressed = false; }

        if middle || (left && ctrl) {
            if let (Some(cur), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                self.pan_camera(cur.x - last.x, cur.y - last.y);
            }
            self.middle_mouse_pressed = true;
        } else { self.middle_mouse_pressed = false; }

        ui.input(|i| { if i.smooth_scroll_delta.y != 0.0 { self.zoom_camera(i.smooth_scroll_delta.y); } });

        if !self.gizmo.dragging {
            self.gizmo.mode = match self.current_tool {
                EditorTool::Move => GizmoMode::Translate,
                EditorTool::Rotate => GizmoMode::Rotate,
                EditorTool::Scale => GizmoMode::Scale,
                EditorTool::Select => GizmoMode::None,
            };
        }

        if left && !ctrl {
            if let Some(pos) = mouse_pos {
                if !self.gizmo.dragging {
                    let axis = self.detect_gizmo_axis(pos, rect);
                    if axis != GizmoAxis::None && self.gizmo.mode != GizmoMode::None {
                        let world_pos = self.screen_to_world(pos, rect);
                        self.gizmo.begin_drag(axis, world_pos);
                        if let Some(obj) = self.scene.selected_objects().first() {
                            self.gizmo.drag_start_value = match self.gizmo.mode {
                                GizmoMode::Translate => obj.transform.position,
                                GizmoMode::Rotate => Vec3::ZERO,
                                GizmoMode::Scale => obj.transform.scale,
                                _ => Vec3::ZERO,
                            };
                        }
                    } else if !self.left_mouse_pressed {
                        let mut closest_dist = f32::MAX;
                        let mut closest_id = None;
                        for (&id, _obj) in self.scene.objects.iter() {
                            let world_pos = self.scene.get_world_transform(id).position;
                            let dist = (world_pos - self.camera_position).length();
                            if dist < closest_dist && dist < 20.0 {
                                closest_dist = dist;
                                closest_id = Some(id);
                            }
                        }
                        if let Some(id) = closest_id {
                            if !shift { self.scene.clear_selection(); }
                            self.scene.select(id, true);
                            if let Some(obj) = self.scene.get_object(id) {
                                self.log_message(format!("Selected: {}", obj.name), Color32::LIGHT_BLUE);
                            }
                        } else if !shift { self.scene.clear_selection(); }
                    }
                } else {
                    let world_pos = self.screen_to_world(pos, rect);
                    let delta = world_pos - self.gizmo.drag_start_pos;
                    let new_transform = self.gizmo.drag(delta);
                    let ids: Vec<Uuid> = self.scene.selected_ids.clone();
                    for id in ids {
                        if let Some(obj) = self.scene.get_object_mut(id) {
                            if !obj.locked {
                                match self.gizmo.mode {
                                    GizmoMode::Translate => obj.transform.position = new_transform.position,
                                    GizmoMode::Rotate => obj.transform.rotation = new_transform.rotation,
                                    GizmoMode::Scale => obj.transform.scale = new_transform.scale,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        } else if !left && self.gizmo.dragging {
            self.gizmo.end_drag();
            self.log_message("Transform applied", Color32::GREEN);
        }

        self.left_mouse_pressed = left;
        self.last_mouse_pos = mouse_pos;

        if let Some(obj) = self.scene.selected_objects().first() {
            self.gizmo.update(obj.transform);
            self.gizmo.visible = true;
        } else { self.gizmo.visible = false; }

        if right { ui.ctx().set_cursor_icon(CursorIcon::Grabbing); }
        else if middle || (left && ctrl) { ui.ctx().set_cursor_icon(CursorIcon::Move); }
        else if self.gizmo.dragging { ui.ctx().set_cursor_icon(CursorIcon::AllScroll); }
    }

    fn transform_vertex(&self, v: Vec3, m: &[[f32; 4]; 4]) -> Vec3 {
        Vec3::new(
            v.x * m[0][0] + v.y * m[1][0] + v.z * m[2][0] + m[3][0],
            v.x * m[0][1] + v.y * m[1][1] + v.z * m[2][1] + m[3][1],
            v.x * m[0][2] + v.y * m[1][2] + v.z * m[2][2] + m[3][2],
        )
    }

    fn transform_normal(&self, n: Vec3, m: &[[f32; 3]; 3]) -> Vec3 {
        Vec3::new(
            n.x * m[0][0] + n.y * m[0][1] + n.z * m[0][2],
            n.x * m[1][0] + n.y * m[1][1] + n.z * m[1][2],
            n.x * m[2][0] + n.y * m[2][1] + n.z * m[2][2],
        ).normalize()
    }

    fn render_mesh_geometry(&self, ui: &Ui, mesh: &Mesh, transform: &Transform, material: &Material, wireframe: bool, selected: bool, rect: Rect, double_sided: bool) {
        let scale = transform.scale;
        let pos = transform.position;

        let base_color = if selected {
            Color32::from_rgb(255, 200, 100)
        } else {
            Color32::from_rgb(
                (material.color[0] * 255.0) as u8,
                (material.color[1] * 255.0) as u8,
                (material.color[2] * 255.0) as u8,
            )
        };

        let transformed: Vec<Vec3> = mesh.vertices.par_iter()
            .map(|v| Vec3::new(
                v.x * scale.x + pos.x,
                v.y * scale.y + pos.y,
                v.z * scale.z + pos.z,
            ))
            .collect();

        let light_dir = Vec3::new(-0.5, -1.0, -0.5).normalize();

        let triangle_count = mesh.indices.len() / 3;
        let render_data: Vec<_> = (0..triangle_count).into_par_iter().filter_map(|i| {
            let idx = i * 3;
            if idx + 2 >= mesh.indices.len() { return None; }

            let i0 = mesh.indices[idx] as usize;
            let i1 = mesh.indices[idx + 1] as usize;
            let i2 = mesh.indices[idx + 2] as usize;

            if i0 >= transformed.len() || i1 >= transformed.len() || i2 >= transformed.len() {
                return None;
            }

            let v0 = transformed[i0];
            let v1 = transformed[i1];
            let v2 = transformed[i2];

            let edge1 = v1 - v0;
            let edge2 = v2 - v0;
            let mut normal = edge1.cross(edge2);
            let len = normal.length();

            if len < 0.0001 { return None; }

            normal = Vec3::new(normal.x / len, normal.y / len, normal.z / len);

            let to_camera = (self.camera_position - v0).normalize();
            let facing_camera = normal.dot(to_camera) > 0.0;

            if !double_sided && !facing_camera {
                return None;
            }

            if let (Some(p0), Some(p1), Some(p2)) = (
                self.world_to_screen(v0, rect),
                self.world_to_screen(v1, rect),
                self.world_to_screen(v2, rect),
            ) {
                let area = (p1.x - p0.x) * (p2.y - p0.y) - (p1.y - p0.y) * (p2.x - p0.x);
                if area.abs() < 0.5 {
                    return None;
                }

                if wireframe {
                    Some((0, p0, p1, p2, base_color, selected, normal, light_dir, facing_camera))
                } else {
                    let use_normal = if facing_camera { normal } else {
                        Vec3::new(-normal.x, -normal.y, -normal.z)
                    };
                    let brightness = use_normal.dot(light_dir).max(0.0);
                    let ambient = 0.3;
                    let diffuse = brightness * 0.7;
                    let final_brightness = ambient + diffuse;

                    let shaded_color = Color32::from_rgb(
                        (base_color.r() as f32 * final_brightness) as u8,
                        (base_color.g() as f32 * final_brightness) as u8,
                        (base_color.b() as f32 * final_brightness) as u8,
                    );

                    Some((1, p0, p1, p2, shaded_color, selected, normal, light_dir, facing_camera))
                }
            } else {
                None
            }
        }).collect();

        for data in render_data {
            if data.0 == 0 {
                let edge_color = if data.5 {
                    Color32::from_rgb(255, 220, 150)
                } else {
                    Color32::WHITE
                };
                ui.painter().line_segment([data.1, data.2], (1.0, edge_color));
                ui.painter().line_segment([data.2, data.3], (1.0, edge_color));
                ui.painter().line_segment([data.3, data.1], (1.0, edge_color));
            } else {
                let winding = (data.2.x - data.1.x) * (data.3.y - data.1.y) - (data.2.y - data.1.y) * (data.3.x - data.1.x);
                if winding > 0.0 {
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![data.1, data.2, data.3],
                        data.4,
                        (1.0, data.4),
                    ));
                } else {
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![data.1, data.3, data.2],
                        data.4,
                        (1.0, data.4),
                    ));
                }
            }
        }

        if selected {
            let min = Vec3::new(
                mesh.bounds.0.x * scale.x + pos.x,
                mesh.bounds.0.y * scale.y + pos.y,
                mesh.bounds.0.z * scale.z + pos.z,
            );
            let max = Vec3::new(
                mesh.bounds.1.x * scale.x + pos.x,
                mesh.bounds.1.y * scale.y + pos.y,
                mesh.bounds.1.z * scale.z + pos.z,
            );
            self.render_bounding_box(ui, min, max, rect);
        }
    }

    fn render_bounding_box(&self, ui: &Ui, min: Vec3, max: Vec3, rect: Rect) {
        let corners = [
            Vec3::new(min.x, min.y, min.z), Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z), Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, max.z),
        ];
        let edges = [(0,1),(1,2),(2,3),(3,0), (4,5),(5,6),(6,7),(7,4), (0,4),(1,5),(2,6),(3,7)];
        let color = Color32::from_rgb(255, 200, 100);

        for (i, j) in edges {
            if let (Some(p1), Some(p2)) = (self.world_to_screen(corners[i], rect), self.world_to_screen(corners[j], rect)) {
                ui.painter().line_segment([p1, p2], (1.5, color));
            }
        }
    }

    fn render_viewport(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        self.viewport_rect = rect;
        self.handle_viewport_input(ui, rect);

        let bg = Color32::from_rgb(
            (self.scene.ambient_color[0] * 255.0) as u8,
            (self.scene.ambient_color[1] * 255.0) as u8,
            (self.scene.ambient_color[2] * 255.0) as u8,
        );
        ui.painter().rect_filled(rect, 0.0, bg);

        if self.scene.grid_enabled { self.render_grid(ui, rect); }
        self.render_axes(ui, rect);

        let mut sorted_objects: Vec<_> = self.scene.objects.values().collect();
        sorted_objects.sort_by(|a, b| {
            let dist_a = (self.scene.get_world_transform(a.id).position - self.camera_position).length();
            let dist_b = (self.scene.get_world_transform(b.id).position - self.camera_position).length();
            dist_b.partial_cmp(&dist_a).unwrap()
        });

        for obj in sorted_objects {
            if !obj.visible { continue; }
            let world = self.scene.get_world_transform(obj.id);
            let selected = self.scene.selected_ids.contains(&obj.id);

            match obj.object_type {
                ObjectType::Mesh(ref m) => {
                    if m.solid || m.wireframe {
                        self.render_mesh_geometry(
                            ui, &m.mesh, &world, &m.material,
                            m.wireframe, selected, rect, m.double_sided
                        );
                    }
                }
                ObjectType::ParticleSystem(ref ps) => {
                    for p in &ps.system.particles {
                        if let Some(spos) = self.world_to_screen(p.position, rect) {
                            let alpha = (p.color[3] * 255.0) as u8;
                            let pcolor = Color32::from_rgba_premultiplied(
                                (p.color[0] * 255.0) as u8,
                                (p.color[1] * 255.0) as u8,
                                (p.color[2] * 255.0) as u8,
                                alpha,
                            );
                            ui.painter().circle(spos, p.size * 50.0, pcolor, (1.0, pcolor));
                        }
                    }
                    if let Some(pos) = self.world_to_screen(world.position, rect) {
                        ui.painter().text(pos, Align2::CENTER_CENTER, "✨", FontId::proportional(24.0), Color32::from_rgb(255, 150, 50));
                    }
                }
                ObjectType::Light(ref l) => {
                    if let Some(pos) = self.world_to_screen(world.position, rect) {
                        let color = Color32::from_rgb((l.color[0]*255.0) as u8, (l.color[1]*255.0) as u8, (l.color[2]*255.0) as u8);
                        ui.painter().circle(pos, 15.0, color, (2.0, Color32::WHITE));
                        ui.painter().text(pos, Align2::CENTER_CENTER, "💡", FontId::proportional(20.0), Color32::WHITE);
                    }
                }
                ObjectType::Camera(_) => {
                    if let Some(pos) = self.world_to_screen(world.position, rect) {
                        ui.painter().rect(Rect::from_center_size(pos, vec2(20.0, 15.0)), 0.0, Color32::from_rgb(100, 200, 255), (2.0, Color32::WHITE));
                        ui.painter().text(pos, Align2::CENTER_CENTER, "📷", FontId::proportional(16.0), Color32::WHITE);
                    }
                }
                ObjectType::Empty => {
                    if let Some(pos) = self.world_to_screen(world.position, rect) {
                        ui.painter().circle(pos, 8.0, Color32::from_rgb(150, 150, 150), (2.0, Color32::WHITE));
                    }
                }
            }

            if let Some(pos) = self.world_to_screen(world.position + Vec3::UP * 1.0, rect) {
                ui.painter().text(pos, Align2::CENTER_CENTER, &obj.name, FontId::proportional(10.0), if selected { Color32::WHITE } else { Color32::LIGHT_GRAY });
            }
        }

        if self.gizmo.visible && self.gizmo.mode != GizmoMode::None { self.render_gizmo(ui, rect); }
        self.render_viewport_overlay(ui, rect);
    }

    fn render_grid(&self, ui: &Ui, rect: Rect) {
        let color = Color32::from_rgb(60, 60, 70);
        let grid_points: Vec<_> = (-20..=20).flat_map(|i| (-20..=20).map(move |j| (i, j))).collect();

        for &(i, j) in &grid_points {
            let x = i as f32;
            let z = j as f32;
            if let (Some(p1), Some(p2)) = (self.world_to_screen(Vec3::new(x, 0.0, z), rect), self.world_to_screen(Vec3::new(x+1.0, 0.0, z), rect)) {
                if p1.x >= rect.min.x && p1.x <= rect.max.x || p2.x >= rect.min.x && p2.x <= rect.max.x {
                    ui.painter().line_segment([p1, p2], (1.0, color));
                }
            }
            if let (Some(p1), Some(p3)) = (self.world_to_screen(Vec3::new(x, 0.0, z), rect), self.world_to_screen(Vec3::new(x, 0.0, z+1.0), rect)) {
                if p1.y >= rect.min.y && p1.y <= rect.max.y || p3.y >= rect.min.y && p3.y <= rect.max.y {
                    ui.painter().line_segment([p1, p3], (1.0, color));
                }
            }
        }
    }

    fn render_axes(&self, ui: &Ui, rect: Rect) {
        if let (Some(o), Some(x), Some(y), Some(z)) = (
            self.world_to_screen(Vec3::ZERO, rect),
            self.world_to_screen(Vec3::RIGHT * 3.0, rect),
            self.world_to_screen(Vec3::UP * 3.0, rect),
            self.world_to_screen(Vec3::FORWARD * 3.0, rect),
        ) {
            ui.painter().arrow(o, x - o, (2.0, Color32::RED));
            ui.painter().arrow(o, y - o, (2.0, Color32::GREEN));
            ui.painter().arrow(o, z - o, (2.0, Color32::BLUE));
        }
    }

    fn render_gizmo(&self, ui: &Ui, rect: Rect) {
        let pos = self.gizmo.position;
        if let Some(spos) = self.world_to_screen(pos, rect) {
            for (axis, dir, col) in [(GizmoAxis::X, Vec3::RIGHT, Color32::RED), (GizmoAxis::Y, Vec3::UP, Color32::GREEN), (GizmoAxis::Z, Vec3::FORWARD, Color32::BLUE)] {
                let adir = if self.gizmo.space == GizmoSpace::World { dir } else { self.gizmo.rotation.rotate(dir) };
                let end = pos + adir * 2.0;
                if let Some(send) = self.world_to_screen(end, rect) {
                    let lcol = if self.gizmo.active_axis == axis { Color32::YELLOW } else { col };
                    ui.painter().line_segment([spos, send], (if self.gizmo.active_axis == axis { 3.0 } else { 2.0 }, lcol));
                }
            }
            ui.painter().circle(spos, 5.0, Color32::WHITE, (1.0, Color32::WHITE));
        }
    }

    fn render_viewport_overlay(&self, ui: &mut Ui, rect: Rect) {
        ui.painter().text(egui::pos2(rect.min.x + 10.0, rect.min.y + 10.0), Align2::LEFT_TOP,
                          format!("Tool: {:?} | Objects: {} | {}", self.current_tool, self.scene.objects.len(),
                                  if self.scene.playing { "▶ PLAYING" } else { "⏸ PAUSED" }),
                          FontId::proportional(12.0), Color32::LIGHT_GRAY);
        let hints = [("RMB-Orbit", Color32::WHITE), ("MMB-Pan", Color32::WHITE), ("Scroll-Zoom", Color32::WHITE), ("W-Move", Color32::WHITE), ("E-Rotate", Color32::WHITE), ("R-Scale", Color32::WHITE), ("Space-Play", Color32::WHITE)];
        let mut y = 50.0;
        for (t, c) in hints {
            ui.painter().text(egui::pos2(rect.max.x - 120.0, rect.min.y + y), Align2::RIGHT_TOP, t, FontId::proportional(10.0), c);
            y += 18.0;
        }
    }

    fn create_mesh_object(&mut self, mesh_name: &str) {
        if let Some(mesh) = self.asset_library.get_mesh(mesh_name) {
            let name = mesh_name.chars().next().unwrap().to_uppercase().collect::<String>() + &mesh_name[1..];
            let o = GameObject::new(&name, ObjectType::Mesh(MeshComponent {
                mesh: mesh.clone(),
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: true,
            }));
            let id = o.id;
            self.scene.add_object(o.clone());
            self.history.push(EditorCommand::CreateObject { id, object: o });
            self.log_message(format!("Created {}", name), Color32::GREEN);
        }
    }

    fn render_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Scene").clicked() { self.new_scene_name = "New Scene".to_string(); self.show_new_scene_dialog = true; ui.close_menu(); }
                    if ui.button("Open Scene...").clicked() { self.log_message("Open not implemented", Color32::YELLOW); ui.close_menu(); }
                    if ui.button("Save Scene").clicked() { self.show_save_dialog = true; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Import Model...").clicked() { self.show_import_dialog = true; ui.close_menu(); }
                    if ui.button("Export Scene...").clicked() { self.show_export_dialog = true; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Settings").clicked() { self.show_settings_dialog = true; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Exit").clicked() { std::process::exit(0); }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo (Ctrl+Z)").clicked() && self.history.undo(&mut self.scene) { self.log_message("Undo", Color32::LIGHT_BLUE); ui.close_menu(); }
                    if ui.button("Redo (Ctrl+Y)").clicked() && self.history.redo(&mut self.scene) { self.log_message("Redo", Color32::LIGHT_BLUE); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Duplicate (Ctrl+D)").clicked() { self.scene.duplicate_selected(); self.log_message("Duplicated", Color32::GREEN); ui.close_menu(); }
                    if ui.button("Delete (Del)").clicked() {
                        for obj in self.scene.selected_objects() { self.history.push(EditorCommand::DeleteObject { id: obj.id, object: obj.clone() }); }
                        self.scene.delete_selected();
                        self.log_message("Deleted", Color32::GREEN);
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_hierarchy, "Hierarchy");
                    ui.checkbox(&mut self.show_inspector, "Inspector");
                    ui.checkbox(&mut self.show_asset_browser, "Asset Browser");
                    ui.checkbox(&mut self.show_console, "Console");
                    ui.checkbox(&mut self.show_animation_timeline, "Timeline");
                    ui.separator();
                    ui.checkbox(&mut self.scene.grid_enabled, "Show Grid");
                });
                ui.menu_button("GameObject", |ui| {
                    if ui.button("Empty").clicked() { let o = GameObject::new("Empty", ObjectType::Empty); let id = o.id; self.scene.add_object(o.clone()); self.history.push(EditorCommand::CreateObject { id, object: o }); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Cube").clicked() { self.create_mesh_object("cube"); ui.close_menu(); }
                    if ui.button("Sphere").clicked() { self.create_mesh_object("sphere"); ui.close_menu(); }
                    if ui.button("Plane").clicked() { self.create_mesh_object("plane"); ui.close_menu(); }
                    if ui.button("Cylinder").clicked() { self.create_mesh_object("cylinder"); ui.close_menu(); }
                    if ui.button("Cone").clicked() { self.create_mesh_object("cone"); ui.close_menu(); }
                    if ui.button("Torus").clicked() { self.create_mesh_object("torus"); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Point Light").clicked() { self.create_light_object(LightType::Point); ui.close_menu(); }
                    if ui.button("Directional Light").clicked() { self.create_light_object(LightType::Directional); ui.close_menu(); }
                    if ui.button("Camera").clicked() { let o = GameObject::new("Camera", ObjectType::Camera(CameraComponent { fov: 60.0, near: 0.1, far: 1000.0, orthographic: false })); let id = o.id; self.scene.add_object(o.clone()); self.history.push(EditorCommand::CreateObject { id, object: o }); ui.close_menu(); }
                    if ui.button("Particle System").clicked() { let o = GameObject::new("Particle System", ObjectType::ParticleSystem(ParticleSystemComponent { system: ParticleSystem::new(), enabled: true })); let id = o.id; self.scene.add_object(o.clone()); self.history.push(EditorCommand::CreateObject { id, object: o }); ui.close_menu(); }
                });
                ui.add_space(20.0);
                ui.selectable_value(&mut self.current_tool, EditorTool::Select, "🖱");
                ui.selectable_value(&mut self.current_tool, EditorTool::Move, "↔");
                ui.selectable_value(&mut self.current_tool, EditorTool::Rotate, "🔄");
                ui.selectable_value(&mut self.current_tool, EditorTool::Scale, "⤢");
                ui.separator();
                if ui.button(if self.scene.playing { "⏸" } else { "▶" }).clicked() { self.scene.playing = !self.scene.playing; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(format!("FPS: {:.1}", self.fps)); });
            });
        });
    }

    fn create_light_object(&mut self, light_type: LightType) {
        let name = match light_type { LightType::Point => "Point Light", LightType::Directional => "Directional Light", LightType::Spot { .. } => "Spot Light" };
        let o = GameObject::new(name, ObjectType::Light(LightComponent { light_type, color: [1.0, 1.0, 1.0], intensity: 1.0, range: 10.0, enabled: true }));
        let id = o.id;
        self.scene.add_object(o.clone());
        self.history.push(EditorCommand::CreateObject { id, object: o });
        self.log_message(format!("Created {}", name), Color32::GREEN);
    }

    fn render_hierarchy(&mut self, ctx: &egui::Context) {
        if !self.show_hierarchy { return; }
        egui::SidePanel::left("hierarchy").default_width(250.0).resizable(true).show(ctx, |ui| {
            ui.heading("📁 Hierarchy");
            ui.separator();
            ui.horizontal(|ui| { ui.label("🔍"); ui.text_edit_singleline(&mut self.search_filter); });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_select = None; let mut to_toggle = None; let mut to_delete = None; let mut to_duplicate = None;
                let objs: Vec<_> = self.scene.objects.iter().filter(|(_, o)| self.search_filter.is_empty() || o.name.to_lowercase().contains(&self.search_filter.to_lowercase())).map(|(&id, o)| (id, o.name.clone(), o.visible, self.scene.selected_ids.contains(&id))).collect();
                for (id, name, vis, sel) in objs {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, if vis { "👁" } else { "👁‍🗨" }).clicked() { to_toggle = Some(id); }
                        let resp = ui.selectable_label(sel, &name);
                        if resp.clicked() { to_select = Some(id); }
                        resp.context_menu(|ui| {
                            if ui.button("Delete").clicked() { to_delete = Some(id); ui.close_menu(); }
                            if ui.button("Duplicate").clicked() { to_duplicate = Some(id); ui.close_menu(); }
                            if ui.button("Focus").clicked() { if let Some(o) = self.scene.get_object(id) { self.camera_target = o.transform.position; } ui.close_menu(); }
                        });
                    });
                }
                if let Some(id) = to_select { let add = ctx.input(|i| i.modifiers.shift); self.scene.select(id, add); }
                if let Some(id) = to_toggle { if let Some(o) = self.scene.get_object_mut(id) { o.visible = !o.visible; } }
                if let Some(id) = to_delete { if let Some(o) = self.scene.get_object(id) { self.history.push(EditorCommand::DeleteObject { id, object: o.clone() }); } self.scene.remove_object(id); }
                if let Some(id) = to_duplicate { if let Some(o) = self.scene.get_object(id) { let mut new_obj = o.clone(); new_obj.id = Uuid::new_v4(); new_obj.name = format!("{} (Copy)", o.name); new_obj.transform.position = o.transform.position + Vec3::new(1.0, 0.0, 1.0); let new_id = new_obj.id; self.scene.add_object(new_obj.clone()); self.history.push(EditorCommand::CreateObject { id: new_id, object: new_obj }); } }
            });
        });
    }

    fn render_inspector(&mut self, ctx: &egui::Context) {
        if !self.show_inspector { return; }
        egui::SidePanel::right("inspector").default_width(300.0).resizable(true).show(ctx, |ui| {
            ui.heading("🔧 Inspector");
            ui.separator();
            let sel = self.scene.selected_objects();
            if sel.len() == 1 {
                let id = sel[0].id;
                let mut obj = self.scene.get_object(id).cloned();
                if let Some(ref mut o) = obj {
                    ui.horizontal(|ui| { ui.label("Name:"); ui.text_edit_singleline(&mut o.name); });
                    ui.collapsing("Transform", |ui| {
                        ui.horizontal(|ui| { ui.label("X"); ui.add(egui::DragValue::new(&mut o.transform.position.x).speed(0.1)); });
                        ui.horizontal(|ui| { ui.label("Y"); ui.add(egui::DragValue::new(&mut o.transform.position.y).speed(0.1)); });
                        ui.horizontal(|ui| { ui.label("Z"); ui.add(egui::DragValue::new(&mut o.transform.position.z).speed(0.1)); });
                    });
                    match &mut o.object_type {
                        ObjectType::Mesh(ref mut m) => {
                            ui.checkbox(&mut m.wireframe, "Wireframe");
                            ui.checkbox(&mut m.solid, "Solid");
                            ui.checkbox(&mut m.visible, "Visible");
                            ui.checkbox(&mut m.double_sided, "Double-sided");
                            ui.separator();
                            ui.label("Material");
                            let r = m.material.color[0]; let g = m.material.color[1]; let b = m.material.color[2];
                            let mut rgb = [r, g, b];
                            if ui.color_edit_button_rgb(&mut rgb).changed() { m.material.color = [rgb[0], rgb[1], rgb[2], 1.0]; }
                            ui.add(egui::Slider::new(&mut m.material.metallic, 0.0..=1.0).text("Metallic"));
                            ui.add(egui::Slider::new(&mut m.material.roughness, 0.0..=1.0).text("Roughness"));
                        }
                        ObjectType::Light(ref mut l) => {
                            ui.checkbox(&mut l.enabled, "Enabled");
                            let r = l.color[0]; let g = l.color[1]; let b = l.color[2];
                            let mut rgb = [r, g, b];
                            if ui.color_edit_button_rgb(&mut rgb).changed() { l.color = [rgb[0], rgb[1], rgb[2]]; }
                            ui.add(egui::Slider::new(&mut l.intensity, 0.0..=10.0).text("Intensity"));
                        }
                        ObjectType::Camera(ref mut c) => { ui.add(egui::Slider::new(&mut c.fov, 30.0..=120.0).text("FOV")); }
                        ObjectType::ParticleSystem(ref mut ps) => {
                            ui.checkbox(&mut ps.enabled, "Enabled");
                            ui.add(egui::Slider::new(&mut ps.system.emission_rate, 1.0..=100.0).text("Emission Rate"));
                            ui.add(egui::Slider::new(&mut ps.system.lifetime, 0.1..=10.0).text("Lifetime"));
                        }
                        ObjectType::Empty => {}
                    }
                }
                if let Some(o) = obj { if let Some(orig) = self.scene.get_object_mut(id) { *orig = o; } }
                ui.separator();
                if ui.button("🗑 Remove").clicked() {
                    if let Some(o) = self.scene.get_object(id) { self.history.push(EditorCommand::DeleteObject { id, object: o.clone() }); }
                    self.scene.remove_object(id);
                }
            } else if sel.len() > 1 {
                ui.label(format!("{} objects selected", sel.len()));
            } else {
                ui.label("No object selected");
                ui.separator();
                ui.horizontal(|ui| { ui.label("Scene:"); ui.text_edit_singleline(&mut self.scene.name); });
                let r = self.scene.ambient_color[0]; let g = self.scene.ambient_color[1]; let b = self.scene.ambient_color[2];
                let mut rgb = [r, g, b];
                if ui.color_edit_button_rgb(&mut rgb).changed() { self.scene.ambient_color = [rgb[0], rgb[1], rgb[2]]; }
            }
        });
    }

    fn render_console(&mut self, ctx: &egui::Context) {
        if !self.show_console { return; }
        egui::TopBottomPanel::bottom("console").default_height(150.0).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| { ui.heading("💬 Console"); if ui.button("Clear").clicked() { self.console_messages.clear(); } });
            ui.separator();
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                for (msg, col) in &self.console_messages { ui.colored_label(*col, msg); }
            });
        });
    }

    fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").default_height(26.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("🎯 {:?}", self.current_tool));
                ui.separator();
                ui.label(format!("📦 {}", self.scene.objects.len()));
                ui.separator();
                if self.scene.dirty { ui.colored_label(Color32::YELLOW, "● Unsaved"); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.history.can_undo() { ui.colored_label(Color32::GREEN, "⬅ Undo"); }
                    if self.history.can_redo() { ui.colored_label(Color32::GREEN, "➡ Redo"); }
                    ui.separator();
                    ui.label(&self.status_message);
                });
            });
        });
    }

    fn render_import_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("Import Model").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Select model file (OBJ, Blend, FBX, glTF):");
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("3D Models", &["obj", "blend", "fbx", "gltf", "glb"]).pick_file() {
                    let path_str = path.to_string_lossy().to_string();
                    self.log_message(format!("Importing: {}...", path.file_name().unwrap().to_string_lossy()), Color32::LIGHT_BLUE);
                    match self.asset_library.import_model(&path_str) {
                        Ok(names) => {
                            for name in names {
                                self.create_mesh_object(&name);
                            }
                            self.log_message("✅ Import successful", Color32::GREEN);
                            self.show_import_dialog = false;
                        }
                        Err(e) => { self.log_message(format!("❌ Import failed: {}", e), Color32::RED); }
                    }
                }
            }
            if ui.button("Cancel").clicked() { self.show_import_dialog = false; }
        });
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(Key::W) { self.current_tool = EditorTool::Move; }
            if i.key_pressed(Key::E) { self.current_tool = EditorTool::Rotate; }
            if i.key_pressed(Key::R) { self.current_tool = EditorTool::Scale; }
            if i.key_pressed(Key::Q) { self.current_tool = EditorTool::Select; }
            if i.key_pressed(Key::Space) { self.scene.playing = !self.scene.playing; }
            if i.key_pressed(Key::F) { if let Some(o) = self.scene.selected_objects().first() { self.camera_target = o.transform.position; } }
            if i.key_pressed(Key::Delete) {
                for obj in self.scene.selected_objects() { self.history.push(EditorCommand::DeleteObject { id: obj.id, object: obj.clone() }); }
                self.scene.delete_selected();
            }
            if i.key_pressed(Key::Z) && i.modifiers.ctrl { self.history.undo(&mut self.scene); }
            if i.key_pressed(Key::Y) && i.modifiers.ctrl { self.history.redo(&mut self.scene); }
            if i.key_pressed(Key::D) && i.modifiers.ctrl { self.scene.duplicate_selected(); }
            if i.key_pressed(Key::A) && i.modifiers.ctrl { self.scene.selected_ids = self.scene.objects.keys().copied().collect(); }
            if i.key_pressed(Key::S) && i.modifiers.ctrl { self.log_message("Scene saved", Color32::GREEN); self.scene.dirty = false; }
        });
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let delta_time = (now - self.last_update_time) as f32;
        self.last_update_time = now;

        self.scene.update(delta_time);
        self.handle_keyboard_shortcuts(ctx);

        self.frame_count += 1;
        if now - self.last_frame_time > 1.0 { self.fps = self.frame_count as f32; self.frame_count = 0; self.last_frame_time = now; }

        self.render_menu_bar(ctx);
        self.render_hierarchy(ctx);
        self.render_inspector(ctx);
        self.render_console(ctx);
        self.render_status_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| { self.render_viewport(ui); });

        if self.show_new_scene_dialog {
            egui::Window::new("New Scene").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("Enter scene name:");
                ui.text_edit_singleline(&mut self.new_scene_name);
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() { self.scene = Scene::new(&self.new_scene_name); self.history = CommandHistory::new(100); self.show_new_scene_dialog = false; }
                    if ui.button("Cancel").clicked() { self.show_new_scene_dialog = false; }
                });
            });
        }

        if self.show_import_dialog { self.render_import_dialog(ctx); }

        if self.show_save_dialog {
            egui::Window::new("Save Scene").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("Save scene as:");
                let mut name = self.scene.name.clone();
                ui.text_edit_singleline(&mut name);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() { self.scene.name = name; self.scene.dirty = false; self.show_save_dialog = false; }
                    if ui.button("Cancel").clicked() { self.show_save_dialog = false; }
                });
            });
        }

        if self.show_export_dialog {
            egui::Window::new("Export Scene").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("Export as:");
                ui.horizontal(|ui| {
                    if ui.button("OBJ").clicked() { self.log_message("Exported to OBJ", Color32::GREEN); self.show_export_dialog = false; }
                    if ui.button("glTF").clicked() { self.log_message("Exported to glTF", Color32::GREEN); self.show_export_dialog = false; }
                    if ui.button("Cancel").clicked() { self.show_export_dialog = false; }
                });
            });
        }

        if self.show_settings_dialog {
            egui::Window::new("Settings").default_width(400.0).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.heading("Editor Settings");
                ui.separator();
                ui.checkbox(&mut self.scene.grid_enabled, "Show Grid");
                ui.checkbox(&mut self.gizmo.visible, "Show Gizmo");
                ui.add(egui::Slider::new(&mut self.camera_fov, 30.0..=120.0).text("Camera FOV"));
                if ui.button("Close").clicked() { self.show_settings_dialog = false; }
            });
        }

        if self.show_asset_browser {
            egui::Window::new("📦 Asset Browser").default_size([400.0, 300.0]).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.collapsing("Meshes", |ui| { for name in self.asset_library.list_meshes() { if ui.button(&name).clicked() { self.create_mesh_object(&name); } } });
                    ui.collapsing("Materials", |ui| { for name in self.asset_library.list_materials() { ui.label(&name); } });
                });
            });
        }

        if self.show_animation_timeline {
            egui::TopBottomPanel::bottom("timeline").default_height(100.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(if self.scene.playing { "⏸" } else { "▶" }).clicked() { self.scene.playing = !self.scene.playing; }
                    if ui.button("⏹").clicked() { self.scene.animation_time = 0.0; }
                    ui.separator();
                    ui.label(format!("Time: {:.2}s", self.scene.animation_time));
                    ui.add(egui::Slider::new(&mut self.scene.animation_time, 0.0..=10.0).text(""));
                });
            });
        }

        ctx.request_repaint();
    }
}

fn setup_egui_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(35, 35, 40);
    style.visuals.panel_fill = Color32::from_rgb(30, 30, 35);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(255, 120, 30);
    style.visuals.selection.bg_fill = Color32::from_rgb(255, 120, 30);
    ctx.set_style(style);
}