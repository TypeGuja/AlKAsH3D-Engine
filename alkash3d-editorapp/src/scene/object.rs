//! Игровые объекты и компоненты

use crate::math::{Transform, Vec3, Vec4, AABB};
use uuid::Uuid;
use serde::{Serialize, Deserialize};

// ============================================================
// GameObject
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameObject {
    pub id: Uuid,
    pub name: String,
    pub tag: String,
    pub layer: u32,
    pub transform: Transform,
    pub visible: bool,
    pub locked: bool,
    pub parent: Option<Uuid>,
    pub children: Vec<Uuid>,
    pub components: Vec<Component>,
    pub metadata: ObjectMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectMetadata {
    pub created_at: u64,
    pub modified_at: u64,
    pub author: String,
    pub notes: String,
}

impl GameObject {
    pub fn new(name: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            tag: String::new(),
            layer: 0,
            transform: Transform::identity(),
            visible: true,
            locked: false,
            parent: None,
            children: Vec::new(),
            components: Vec::new(),
            metadata: ObjectMetadata {
                created_at: now,
                modified_at: now,
                author: String::new(),
                notes: String::new(),
            },
        }
    }

    pub fn with_mesh(mut self, asset_id: impl Into<String>) -> Self {
        self.components.push(Component::MeshRenderer(MeshRendererComponent {
            asset_id: asset_id.into(),
            materials: Vec::new(),
            cast_shadows: true,
            receive_shadows: true,
            visible: true,
            wireframe: false,
        }));
        self
    }

    pub fn with_light(mut self, light_type: LightType) -> Self {
        self.components.push(Component::Light(LightComponent {
            light_type,
            color: Vec3::ONE,
            intensity: 1.0,
            range: 10.0,
            cast_shadows: true,
            enabled: true,
        }));
        self
    }

    pub fn get_component<T: 'static>(&self) -> Option<&T> {
        self.components.iter().find_map(|c| c.as_any().downcast_ref::<T>())
    }

    pub fn get_component_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.components.iter_mut().find_map(|c| c.as_any_mut().downcast_mut::<T>())
    }

    pub fn has_component<T: 'static>(&self) -> bool {
        self.components.iter().any(|c| c.as_any().is::<T>())
    }

    pub fn world_transform(&self) -> Mat4 {
        self.transform.to_matrix()
    }

    pub fn world_position(&self) -> Vec3 {
        self.transform.translation
    }
}

// ============================================================
// Components
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Component {
    Transform(TransformComponent),
    MeshRenderer(MeshRendererComponent),
    MeshFilter(MeshFilterComponent),
    Light(LightComponent),
    Camera(CameraComponent),
    Collider(ColliderComponent),
    Rigidbody(RigidbodyComponent),
    Script(ScriptComponent),
    AudioSource(AudioSourceComponent),
    ParticleSystem(ParticleSystemComponent),
}

use std::any::Any;

impl Component {
    fn as_any(&self) -> &dyn Any {
        match self {
            Self::Transform(c) => c,
            Self::MeshRenderer(c) => c,
            Self::MeshFilter(c) => c,
            Self::Light(c) => c,
            Self::Camera(c) => c,
            Self::Collider(c) => c,
            Self::Rigidbody(c) => c,
            Self::Script(c) => c,
            Self::AudioSource(c) => c,
            Self::ParticleSystem(c) => c,
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        match self {
            Self::Transform(c) => c,
            Self::MeshRenderer(c) => c,
            Self::MeshFilter(c) => c,
            Self::Light(c) => c,
            Self::Camera(c) => c,
            Self::Collider(c) => c,
            Self::Rigidbody(c) => c,
            Self::Script(c) => c,
            Self::AudioSource(c) => c,
            Self::ParticleSystem(c) => c,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformComponent {
    pub local_position: Vec3,
    pub local_rotation: Quat,
    pub local_scale: Vec3,
}

impl Default for TransformComponent {
    fn default() -> Self {
        Self {
            local_position: Vec3::ZERO,
            local_rotation: Quat::IDENTITY,
            local_scale: Vec3::ONE,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshRendererComponent {
    pub asset_id: String,
    pub materials: Vec<MaterialSlot>,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    pub visible: bool,
    pub wireframe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialSlot {
    pub material_id: String,
    pub submesh_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshFilterComponent {
    pub asset_id: String,
    pub bounds: AABB,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightComponent {
    pub light_type: LightType,
    pub color: Vec3,
    pub intensity: f32,
    pub range: f32,
    pub cast_shadows: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LightType {
    Point,
    Spot { inner_angle: f32, outer_angle: f32 },
    Directional,
    Area { width: f32, height: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraComponent {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub orthographic: bool,
    pub ortho_size: f32,
    pub is_main: bool,
    pub priority: i32,
    pub clear_flags: CameraClearFlags,
    pub background_color: Vec4,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CameraClearFlags {
    Skybox,
    SolidColor,
    DepthOnly,
    DontClear,
}

impl Default for CameraComponent {
    fn default() -> Self {
        Self {
            fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            orthographic: false,
            ortho_size: 10.0,
            is_main: true,
            priority: 0,
            clear_flags: CameraClearFlags::SolidColor,
            background_color: Vec4::new(0.1, 0.1, 0.15, 1.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderComponent {
    pub shape: ColliderShape,
    pub is_trigger: bool,
    pub enabled: bool,
    pub material: PhysicsMaterial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColliderShape {
    Box { size: Vec3 },
    Sphere { radius: f32 },
    Capsule { radius: f32, height: f32 },
    Mesh { convex: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsMaterial {
    pub friction: f32,
    pub restitution: f32,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            friction: 0.5,
            restitution: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigidbodyComponent {
    pub mass: f32,
    pub drag: f32,
    pub angular_drag: f32,
    pub use_gravity: bool,
    pub is_kinematic: bool,
    pub constraints: RigidbodyConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigidbodyConstraints {
    pub freeze_position_x: bool,
    pub freeze_position_y: bool,
    pub freeze_position_z: bool,
    pub freeze_rotation_x: bool,
    pub freeze_rotation_y: bool,
    pub freeze_rotation_z: bool,
}

impl Default for RigidbodyConstraints {
    fn default() -> Self {
        Self {
            freeze_position_x: false,
            freeze_position_y: false,
            freeze_position_z: false,
            freeze_rotation_x: false,
            freeze_rotation_y: false,
            freeze_rotation_z: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptComponent {
    pub script_path: String,
    pub enabled: bool,
    pub variables: std::collections::HashMap<String, ScriptValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceComponent {
    pub clip_id: String,
    pub volume: f32,
    pub pitch: f32,
    pub loop_: bool,
    pub play_on_awake: bool,
    pub spatial_blend: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub enabled: bool,
}

impl Default for AudioSourceComponent {
    fn default() -> Self {
        Self {
            clip_id: String::new(),
            volume: 1.0,
            pitch: 1.0,
            loop_: false,
            play_on_awake: true,
            spatial_blend: 1.0,
            min_distance: 1.0,
            max_distance: 50.0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleSystemComponent {
    pub asset_id: String,
    pub enabled: bool,
    pub play_on_awake: bool,
    pub looping: bool,
    pub start_delay: f32,
    pub start_lifetime: f32,
    pub start_speed: f32,
    pub start_size: f32,
    pub start_color: Vec4,
    pub max_particles: u32,
    pub emission_rate: f32,
    pub gravity_modifier: f32,
}

impl Default for ParticleSystemComponent {
    fn default() -> Self {
        Self {
            asset_id: String::new(),
            enabled: true,
            play_on_awake: true,
            looping: true,
            start_delay: 0.0,
            start_lifetime: 5.0,
            start_speed: 5.0,
            start_size: 1.0,
            start_color: Vec4::ONE,
            max_particles: 1000,
            emission_rate: 10.0,
            gravity_modifier: 0.0,
        }
    }
}

// Импорты для Quat и Mat4
use glam::{Quat, Mat4, Vec2};